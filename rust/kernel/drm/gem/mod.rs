// SPDX-License-Identifier: GPL-2.0 OR MIT

//! DRM GEM API
//!
//! C header: [`include/linux/drm/drm_gem.h`](../../../../include/linux/drm/drm_gem.h)

#[cfg(CONFIG_DRM_GEM_SHMEM_HELPER = "y")]
pub mod shmem;

use alloc::boxed::Box;

use crate::{
    bindings,
    drm::{device, drv, file},
    error::{to_result, Result},
    prelude::*,
};
use core::{mem, mem::ManuallyDrop, ops::Deref, ops::DerefMut};

/// GEM object functions, which must be implemented by drivers.
pub trait BaseDriverObject<T: BaseObject>: Sync + Send + Sized {
    /// Create a new driver data object for a GEM object of a given size.
    fn new(dev: &device::Device<T::Driver>, size: usize) -> Result<Self>;

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

    /// Returns a reference to the raw `drm_gem_object` structure, which must be valid as long as
    /// this owning object is valid.
    fn mut_gem_obj(&mut self) -> &mut bindings::drm_gem_object;

    /// Converts a pointer to a `drm_gem_object` into a pointer to this type.
    fn from_gem_obj(obj: *mut bindings::drm_gem_object) -> *mut Self;
}

/// Trait which must be implemented by drivers using base GEM objects.
pub trait DriverObject: BaseDriverObject<Object<Self>> {
    /// Parent `Driver` for this object.
    type Driver: drv::Driver;
}

unsafe extern "C" fn free_callback<T: DriverObject>(obj: *mut bindings::drm_gem_object) {
    // SAFETY: All of our objects are Object<T>.
    let this = crate::container_of!(obj, Object<T>, obj) as *mut Object<T>;

    // SAFETY: The pointer we got has to be valid
    unsafe { bindings::drm_gem_object_release(obj) };

    // SAFETY: All of our objects are allocated via Box<>, and we're in the
    // free callback which guarantees this object has zero remaining references,
    // so we can drop it
    unsafe { Box::from_raw(this) };
}

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
        Err(e) => e.to_kernel_errno(),
        Ok(()) => 0,
    }
}

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

    fn mut_gem_obj(&mut self) -> &mut bindings::drm_gem_object {
        &mut self.obj
    }

    fn from_gem_obj(obj: *mut bindings::drm_gem_object) -> *mut Object<T> {
        crate::container_of!(obj, Object<T>, obj) as *mut Object<T>
    }
}

/// Base operations shared by all GEM object classes
pub trait BaseObject: IntoGEMObject {
    /// Returns the size of the object in bytes.
    fn size(&self) -> usize {
        self.gem_obj().size
    }

    /// Sets the exportable flag, which controls whether the object can be exported via PRIME.
    fn set_exportable(&mut self, exportable: bool) {
        self.mut_gem_obj().exportable = exportable;
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
            // TODO: is this threadsafe?
            bindings::drm_gem_create_mmap_offset(self.gem_obj() as *const _ as *mut _)
        })?;
        Ok(unsafe {
            bindings::drm_vma_node_offset_addr(&self.gem_obj().vma_node as *const _ as *mut _)
        })
    }
}

impl<T: IntoGEMObject> BaseObject for T {}

/// A base GEM object.
#[repr(C)]
pub struct Object<T: DriverObject> {
    obj: bindings::drm_gem_object,
    // The DRM core ensures the Device exists as long as its objects exist, so we don't need to
    // manage the reference count here.
    dev: ManuallyDrop<device::Device<T::Driver>>,
    inner: T,
}

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
        vm_ops: core::ptr::null_mut(),
    };

    /// Create a new GEM object.
    pub fn new(dev: &device::Device<T::Driver>, size: usize) -> Result<UniqueObjectRef<Self>> {
        let mut obj: Box<Self> = Box::try_new(Self {
            // SAFETY: This struct is expected to be zero-initialized
            obj: unsafe { mem::zeroed() },
            // SAFETY: The drm subsystem guarantees that the drm_device will live as long as
            // the GEM object lives, so we can conjure a reference out of thin air.
            dev: ManuallyDrop::new(unsafe { device::Device::from_raw(dev.ptr) }),
            inner: T::new(dev, size)?,
        })?;

        obj.obj.funcs = &Self::OBJECT_FUNCS;
        to_result(unsafe {
            bindings::drm_gem_object_init(dev.raw() as *mut _, &mut obj.obj, size)
        })?;

        let obj_ref = UniqueObjectRef {
            ptr: Box::leak(obj),
        };

        Ok(obj_ref)
    }

    /// Returns the `Device` that owns this GEM object.
    pub fn dev(&self) -> &device::Device<T::Driver> {
        &self.dev
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
        prime_handle_to_fd: Some(bindings::drm_gem_prime_handle_to_fd),
        prime_fd_to_handle: Some(bindings::drm_gem_prime_fd_to_handle),
        gem_prime_import: None,
        gem_prime_import_sg_table: None,
        gem_prime_mmap: Some(bindings::drm_gem_prime_mmap),
        dumb_create: None,
        dumb_map_offset: None,
        dumb_destroy: None,
    };
}

/// A reference-counted shared reference to a base GEM object.
pub struct ObjectRef<T: IntoGEMObject> {
    // Invariant: the pointer is valid and initialized, and this ObjectRef owns a reference to it.
    ptr: *const T,
}

/// SAFETY: GEM object references are safe to share between threads.
unsafe impl<T: IntoGEMObject> Send for ObjectRef<T> {}
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

pub(super) fn create_fops() -> bindings::file_operations {
    bindings::file_operations {
        owner: core::ptr::null_mut(),
        open: Some(bindings::drm_open),
        release: Some(bindings::drm_release),
        unlocked_ioctl: Some(bindings::drm_ioctl),
        #[cfg(CONFIG_COMPAT)]
        compat_ioctl: Some(bindings::drm_compat_ioctl),
        #[cfg(not(CONFIG_COMPAT))]
        compat_ioctl: None,
        poll: Some(bindings::drm_poll),
        read: Some(bindings::drm_read),
        llseek: Some(bindings::noop_llseek),
        mmap: Some(bindings::drm_gem_mmap),
        ..Default::default()
    }
}
