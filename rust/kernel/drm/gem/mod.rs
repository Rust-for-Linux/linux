// SPDX-License-Identifier: GPL-2.0 OR MIT

//! DRM GEM API
//!
//! C header: [`include/linux/drm/drm_gem.h`](srctree/include/linux/drm/drm_gem.h)

use crate::{
    alloc::flags::*,
    bindings,
    drm::{device, drv, file},
    error::{to_result, Result},
    prelude::*,
};
use core::{marker::PhantomPinned, mem, ops::Deref, ops::DerefMut};

/// GEM object functions, which must be implemented by drivers.
pub trait BaseDriverObject<T: BaseObject>: Sync + Send + Sized {
    /// Create a new driver data object for a GEM object of a given size.
    fn new(dev: &device::Device<T::Driver>, size: usize) -> impl PinInit<Self, Error>;

    /// Open a new handle to an existing object, associated with a File.
    fn open(
        _obj: &<<T as IntoGEMObject>::Driver as drv::Driver>::Object,
        _file: &file::File<<<T as IntoGEMObject>::Driver as drv::Driver>::File>,
    ) -> Result {
        Ok(())
    }

    /// Close a handle to an existing object, associated with a File.
    fn close(
        _obj: &<<T as IntoGEMObject>::Driver as drv::Driver>::Object,
        _file: &file::File<<<T as IntoGEMObject>::Driver as drv::Driver>::File>,
    ) {
    }
}

/// Trait that represents a GEM object subtype
pub trait IntoGEMObject: Sized + crate::private::Sealed {
    /// Owning driver for this type
    type Driver: drv::Driver;

    /// Returns a reference to the raw `drm_gem_object` structure, which must be valid as long as
    /// this owning object is valid.
    fn gem_obj(&self) -> &bindings::drm_gem_object;

    /// Converts a pointer to a `drm_gem_object` into a pointer to this type.
    fn from_gem_obj(obj: *mut bindings::drm_gem_object) -> *mut Self;
}

/// Trait which must be implemented by drivers using base GEM objects.
pub trait DriverObject: BaseDriverObject<Object<Self>> {
    /// Parent `Driver` for this object.
    type Driver: drv::Driver;
}

#[allow(clippy::missing_safety_doc)]
unsafe extern "C" fn free_callback<T: DriverObject>(obj: *mut bindings::drm_gem_object) {
    // SAFETY: All of our objects are Object<T>.
    let this = unsafe { crate::container_of!(obj, Object<T>, obj) } as *mut Object<T>;

    // SAFETY: The pointer we got has to be valid
    unsafe { bindings::drm_gem_object_release(obj) };

    // SAFETY: All of our objects are allocated via KBox<>, and we're in the
    // free callback which guarantees this object has zero remaining references,
    // so we can drop it
    unsafe {
        let _ = KBox::from_raw(this);
    };
}

#[allow(clippy::missing_safety_doc)]
unsafe extern "C" fn open_callback<T: BaseDriverObject<U>, U: BaseObject>(
    raw_obj: *mut bindings::drm_gem_object,
    raw_file: *mut bindings::drm_file,
) -> core::ffi::c_int {
    // SAFETY: The pointer we got has to be valid.
    let file = unsafe {
        file::File::<<<U as IntoGEMObject>::Driver as drv::Driver>::File>::from_raw(raw_file)
    };
    let obj =
        <<<U as IntoGEMObject>::Driver as drv::Driver>::Object as IntoGEMObject>::from_gem_obj(
            raw_obj,
        );

    // SAFETY: from_gem_obj() returns a valid pointer as long as the type is
    // correct and the raw_obj we got is valid.
    match T::open(unsafe { &*obj }, &file) {
        Err(e) => e.to_errno(),
        Ok(()) => 0,
    }
}

#[allow(clippy::missing_safety_doc)]
unsafe extern "C" fn close_callback<T: BaseDriverObject<U>, U: BaseObject>(
    raw_obj: *mut bindings::drm_gem_object,
    raw_file: *mut bindings::drm_file,
) {
    // SAFETY: The pointer we got has to be valid.
    let file = unsafe {
        file::File::<<<U as IntoGEMObject>::Driver as drv::Driver>::File>::from_raw(raw_file)
    };
    let obj =
        <<<U as IntoGEMObject>::Driver as drv::Driver>::Object as IntoGEMObject>::from_gem_obj(
            raw_obj,
        );

    // SAFETY: from_gem_obj() returns a valid pointer as long as the type is
    // correct and the raw_obj we got is valid.
    T::close(unsafe { &*obj }, &file);
}

impl<T: DriverObject> IntoGEMObject for Object<T> {
    type Driver = T::Driver;

    fn gem_obj(&self) -> &bindings::drm_gem_object {
        &self.obj
    }

    fn from_gem_obj(obj: *mut bindings::drm_gem_object) -> *mut Object<T> {
        // SAFETY: All of our objects are Object<T>.
        unsafe { crate::container_of!(obj, Object<T>, obj) as *mut Object<T> }
    }
}

/// Base operations shared by all GEM object classes
pub trait BaseObject: IntoGEMObject {
    /// Returns the size of the object in bytes.
    fn size(&self) -> usize {
        self.gem_obj().size
    }

    /// Creates a new reference to the object.
    fn reference(&self) -> ObjectRef<Self> {
        // SAFETY: Having a reference to an Object implies holding a GEM reference
        unsafe {
            bindings::drm_gem_object_get(self.gem_obj() as *const _ as *mut _);
        }
        ObjectRef {
            ptr: self as *const _,
        }
    }

    /// Creates a new handle for the object associated with a given `File`
    /// (or returns an existing one).
    fn create_handle(
        &self,
        file: &file::File<<<Self as IntoGEMObject>::Driver as drv::Driver>::File>,
    ) -> Result<u32> {
        let mut handle: u32 = 0;
        // SAFETY: The arguments are all valid per the type invariants.
        to_result(unsafe {
            bindings::drm_gem_handle_create(
                file.raw() as *mut _,
                self.gem_obj() as *const _ as *mut _,
                &mut handle,
            )
        })?;
        Ok(handle)
    }

    /// Looks up an object by its handle for a given `File`.
    fn lookup_handle(
        file: &file::File<<<Self as IntoGEMObject>::Driver as drv::Driver>::File>,
        handle: u32,
    ) -> Result<ObjectRef<Self>> {
        // SAFETY: The arguments are all valid per the type invariants.
        let ptr = unsafe { bindings::drm_gem_object_lookup(file.raw() as *mut _, handle) };

        if ptr.is_null() {
            Err(ENOENT)
        } else {
            Ok(ObjectRef {
                ptr: ptr as *const _,
            })
        }
    }

    /// Creates an mmap offset to map the object from userspace.
    fn create_mmap_offset(&self) -> Result<u64> {
        // SAFETY: The arguments are valid per the type invariant.
        to_result(unsafe {
            bindings::drm_gem_create_mmap_offset(self.gem_obj() as *const _ as *mut _)
        })?;

        // SAFETY: The arguments are valid per the type invariant.
        Ok(unsafe {
            bindings::drm_vma_node_offset_addr(&self.gem_obj().vma_node as *const _ as *mut _)
        })
    }
}

impl<T: IntoGEMObject> BaseObject for T {}

/// A base GEM object.
#[repr(C)]
#[pin_data]
pub struct Object<T: DriverObject> {
    obj: bindings::drm_gem_object,
    // The DRM core ensures the Device exists as long as its objects exist, so we don't need to
    // manage the reference count here.
    dev: *const bindings::drm_device,
    #[pin]
    inner: T,
    #[pin]
    _p: PhantomPinned,
}

// SAFETY: This struct is safe to zero-initialize
unsafe impl init::Zeroable for bindings::drm_gem_object {}

impl<T: DriverObject> Object<T> {
    /// The size of this object's structure.
    pub const SIZE: usize = mem::size_of::<Self>();

    const OBJECT_FUNCS: bindings::drm_gem_object_funcs = bindings::drm_gem_object_funcs {
        free: Some(free_callback::<T>),
        open: Some(open_callback::<T, Object<T>>),
        close: Some(close_callback::<T, Object<T>>),
        print_info: None,
        export: None,
        pin: None,
        unpin: None,
        get_sg_table: None,
        vmap: None,
        vunmap: None,
        mmap: None,
        status: None,
        vm_ops: core::ptr::null_mut(),
        evict: None,
        rss: None,
    };

    /// Create a new GEM object.
    pub fn new(dev: &device::Device<T::Driver>, size: usize) -> Result<Pin<UniqueObjectRef<Self>>> {
        let obj: Pin<KBox<Self>> = KBox::pin_init(
            try_pin_init!(Self {
                // SAFETY: This struct is expected to be zero-initialized
                obj: bindings::drm_gem_object {
                    funcs: &Self::OBJECT_FUNCS,
                    ..Default::default()
                },
                inner <- T::new(dev, size),
                // SAFETY: The drm subsystem guarantees that the drm_device will live as long as
                // the GEM object lives, so we can conjure a reference out of thin air.
                dev: dev.as_raw(),
                _p: PhantomPinned
            }),
            GFP_KERNEL,
        )?;

        // SAFETY: The arguments are all valid per the type invariants.
        to_result(unsafe {
            bindings::drm_gem_object_init(dev.as_raw(), &obj.obj as *const _ as *mut _, size)
        })?;

        // SAFETY: We never move out of self
        let obj_ref = unsafe {
            Pin::new_unchecked(UniqueObjectRef {
                // SAFETY: We never move out of the Box
                ptr: KBox::leak(Pin::into_inner_unchecked(obj)),
                _p: PhantomPinned,
            })
        };

        Ok(obj_ref)
    }

    /// Returns the `Device` that owns this GEM object.
    pub fn dev(&self) -> &device::Device<T::Driver> {
        // SAFETY: The drm subsystem guarantees that the drm_device will live as long as
        // the GEM object lives, so we can just borrow from the raw pointer.
        unsafe { device::Device::borrow(self.dev) }
    }
}

impl<T: DriverObject> crate::private::Sealed for Object<T> {}

impl<T: DriverObject> Deref for Object<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<T: DriverObject> DerefMut for Object<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl<T: DriverObject> drv::AllocImpl for Object<T> {
    const ALLOC_OPS: drv::AllocOps = drv::AllocOps {
        gem_create_object: None,
        prime_handle_to_fd: None,
        prime_fd_to_handle: None,
        gem_prime_import: None,
        gem_prime_import_sg_table: None,
        dumb_create: None,
        dumb_map_offset: None,
    };
}

/// A reference-counted shared reference to a base GEM object.
pub struct ObjectRef<T: IntoGEMObject> {
    // Invariant: the pointer is valid and initialized, and this ObjectRef owns a reference to it.
    ptr: *const T,
}

impl<T: IntoGEMObject> ObjectRef<T> {
    /// Downgrade this reference to a shared reference.
    pub fn from_pinned_unique(pin: Pin<UniqueObjectRef<T>>) -> Self {
        // SAFETY: A (shared) `ObjectRef` doesn't need to be pinned, since it doesn't allow us to
        // optain a mutable reference.
        let uq = unsafe { Pin::into_inner_unchecked(pin) };

        uq.into_ref()
    }
}

/// SAFETY: GEM object references are safe to send between threads.
unsafe impl<T: IntoGEMObject> Send for ObjectRef<T> {}
/// SAFETY: GEM object references are safe to share between threads.
unsafe impl<T: IntoGEMObject> Sync for ObjectRef<T> {}

impl<T: IntoGEMObject> Clone for ObjectRef<T> {
    fn clone(&self) -> Self {
        self.reference()
    }
}

impl<T: IntoGEMObject> Drop for ObjectRef<T> {
    fn drop(&mut self) {
        // SAFETY: Having an ObjectRef implies holding a GEM reference.
        // The free callback will take care of deallocation.
        unsafe {
            bindings::drm_gem_object_put((*self.ptr).gem_obj() as *const _ as *mut _);
        }
    }
}

impl<T: IntoGEMObject> Deref for ObjectRef<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // SAFETY: The pointer is valid per the invariant
        unsafe { &*self.ptr }
    }
}

/// A unique reference to a base GEM object.
pub struct UniqueObjectRef<T: IntoGEMObject> {
    // Invariant: the pointer is valid and initialized, and this ObjectRef owns the only reference
    // to it.
    ptr: *mut T,
    _p: PhantomPinned,
}

impl<T: IntoGEMObject> UniqueObjectRef<T> {
    /// Downgrade this reference to a shared reference.
    pub fn into_ref(self) -> ObjectRef<T> {
        let ptr = self.ptr as *const _;
        core::mem::forget(self);

        ObjectRef { ptr }
    }
}

impl<T: IntoGEMObject> Drop for UniqueObjectRef<T> {
    fn drop(&mut self) {
        // SAFETY: Having a UniqueObjectRef implies holding a GEM
        // reference. The free callback will take care of deallocation.
        unsafe {
            bindings::drm_gem_object_put((*self.ptr).gem_obj() as *const _ as *mut _);
        }
    }
}

impl<T: IntoGEMObject> Deref for UniqueObjectRef<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        // SAFETY: The pointer is valid per the invariant
        unsafe { &*self.ptr }
    }
}

impl<T: IntoGEMObject> DerefMut for UniqueObjectRef<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // SAFETY: The pointer is valid per the invariant
        unsafe { &mut *self.ptr }
    }
}

pub(super) const fn create_fops() -> bindings::file_operations {
    // SAFETY: As by the type invariant, it is safe to initialize `bindings::file_operations`
    // zeroed.
    let mut fops: bindings::file_operations = unsafe { core::mem::zeroed() };

    fops.owner = core::ptr::null_mut();
    fops.open = Some(bindings::drm_open);
    fops.release = Some(bindings::drm_release);
    fops.unlocked_ioctl = Some(bindings::drm_ioctl);
    #[cfg(CONFIG_COMPAT)]
    {
        fops.compat_ioctl = Some(bindings::drm_compat_ioctl);
    }
    fops.poll = Some(bindings::drm_poll);
    fops.read = Some(bindings::drm_read);
    fops.llseek = Some(bindings::noop_llseek);
    fops.mmap = Some(bindings::drm_gem_mmap);
    fops.fop_flags = bindings::FOP_UNSIGNED_OFFSET;

    fops
}
