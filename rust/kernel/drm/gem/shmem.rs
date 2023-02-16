// SPDX-License-Identifier: GPL-2.0

//! DRM GEM shmem helper objects
//!
//! C header: [`include/linux/drm/drm_gem_shmem_helper.h`](../../../../include/linux/drm/drm_gem_shmem_helper.h)

use crate::drm::{device, drv, gem};
use crate::{
    error::{from_kernel_err_ptr, to_result},
    prelude::*,
};
use core::{
    marker::PhantomData,
    mem,
    mem::{ManuallyDrop, MaybeUninit},
    ops::{Deref, DerefMut},
    ptr::addr_of_mut,
    slice,
};

use gem::BaseObject;

/// Trait which must be implemented by drivers using shmem-backed GEM objects.
pub trait DriverObject: gem::BaseDriverObject<Object<Self>> {
    /// Parent `Driver` for this object.
    type Driver: drv::Driver;
}

// FIXME: This is terrible and I don't know how to avoid it
#[cfg(CONFIG_NUMA)]
macro_rules! vm_numa_fields {
    ( $($field:ident: $val:expr),* $(,)? ) => {
        bindings::vm_operations_struct {
            $( $field: $val ),*,
            set_policy: None,
            get_policy: None,
        }
    }
}

#[cfg(not(CONFIG_NUMA))]
macro_rules! vm_numa_fields {
    ( $($field:ident: $val:expr),* $(,)? ) => {
        bindings::vm_operations_struct {
            $( $field: $val ),*
        }
    }
}

const SHMEM_VM_OPS: bindings::vm_operations_struct = vm_numa_fields! {
    open: Some(bindings::drm_gem_shmem_vm_open),
    close: Some(bindings::drm_gem_shmem_vm_close),
    may_split: None,
    mremap: None,
    mprotect: None,
    fault: Some(bindings::drm_gem_shmem_fault),
    huge_fault: None,
    map_pages: None,
    pagesize: None,
    page_mkwrite: None,
    pfn_mkwrite: None,
    access: None,
    name: None,
    find_special_page: None,
};

/// A shmem-backed GEM object.
#[repr(C)]
pub struct Object<T: DriverObject> {
    obj: bindings::drm_gem_shmem_object,
    // The DRM core ensures the Device exists as long as its objects exist, so we don't need to
    // manage the reference count here.
    dev: ManuallyDrop<device::Device<T::Driver>>,
    inner: T,
}

unsafe extern "C" fn gem_create_object<T: DriverObject>(
    raw_dev: *mut bindings::drm_device,
    size: usize,
) -> *mut bindings::drm_gem_object {
    // SAFETY: GEM ensures the device lives as long as its objects live,
    // so we can conjure up a reference from thin air and never drop it.
    let dev = ManuallyDrop::new(unsafe { device::Device::from_raw(raw_dev) });

    let inner = match T::new(&*dev, size) {
        Ok(v) => v,
        Err(e) => return e.to_ptr(),
    };

    let p = unsafe {
        bindings::krealloc(
            core::ptr::null(),
            Object::<T>::SIZE,
            bindings::GFP_KERNEL | bindings::__GFP_ZERO,
        ) as *mut Object<T>
    };

    if p.is_null() {
        return ENOMEM.to_ptr();
    }

    // SAFETY: p is valid as long as the alloc succeeded
    unsafe {
        addr_of_mut!((*p).dev).write(dev);
        addr_of_mut!((*p).inner).write(inner);
    }

    // SAFETY: drm_gem_shmem_object is safe to zero-init, and
    // the rest of Object has been initialized
    let new: &mut Object<T> = unsafe { &mut *(p as *mut _) };

    new.obj.base.funcs = &Object::<T>::VTABLE;
    &mut new.obj.base
}

unsafe extern "C" fn free_callback<T: DriverObject>(obj: *mut bindings::drm_gem_object) {
    // SAFETY: All of our objects are Object<T>.
    let p = crate::container_of!(obj, Object<T>, obj) as *mut Object<T>;

    // SAFETY: p is never used after this
    unsafe {
        core::ptr::drop_in_place(&mut (*p).inner);
    }

    // SAFETY: This pointer has to be valid, since p is valid
    unsafe {
        bindings::drm_gem_shmem_free(&mut (*p).obj);
    }
}

impl<T: DriverObject> Object<T> {
    /// The size of this object's structure.
    const SIZE: usize = mem::size_of::<Self>();

    /// `drm_gem_object_funcs` vtable suitable for GEM shmem objects.
    const VTABLE: bindings::drm_gem_object_funcs = bindings::drm_gem_object_funcs {
        free: Some(free_callback::<T>),
        open: Some(super::open_callback::<T, Object<T>>),
        close: Some(super::close_callback::<T, Object<T>>),
        print_info: Some(bindings::drm_gem_shmem_object_print_info),
        export: None,
        pin: Some(bindings::drm_gem_shmem_object_pin),
        unpin: Some(bindings::drm_gem_shmem_object_unpin),
        get_sg_table: Some(bindings::drm_gem_shmem_object_get_sg_table),
        vmap: Some(bindings::drm_gem_shmem_object_vmap),
        vunmap: Some(bindings::drm_gem_shmem_object_vunmap),
        mmap: Some(bindings::drm_gem_shmem_object_mmap),
        vm_ops: &SHMEM_VM_OPS,
    };

    // SAFETY: Must only be used with DRM functions that are thread-safe
    unsafe fn mut_shmem(&self) -> *mut bindings::drm_gem_shmem_object {
        &self.obj as *const _ as *mut _
    }

    /// Create a new shmem-backed DRM object of the given size.
    pub fn new(dev: &device::Device<T::Driver>, size: usize) -> Result<gem::UniqueObjectRef<Self>> {
        // SAFETY: This function can be called as long as the ALLOC_OPS are set properly
        // for this driver, and the gem_create_object is called.
        let p = unsafe { bindings::drm_gem_shmem_create(dev.raw() as *mut _, size) };
        let p = crate::container_of!(p, Object<T>, obj) as *mut _;

        // SAFETY: The gem_create_object callback ensures this is a valid Object<T>,
        // so we can take a unique reference to it.
        let obj_ref = gem::UniqueObjectRef { ptr: p };

        Ok(obj_ref)
    }

    /// Returns the `Device` that owns this GEM object.
    pub fn dev(&self) -> &device::Device<T::Driver> {
        &self.dev
    }

    /// Creates (if necessary) and returns a scatter-gather table of DMA pages for this object.
    ///
    /// This will pin the object in memory.
    pub fn sg_table(&self) -> Result<SGTable<T>> {
        // SAFETY: drm_gem_shmem_get_pages_sgt is thread-safe.
        let sgt = from_kernel_err_ptr(unsafe {
            bindings::drm_gem_shmem_get_pages_sgt(self.mut_shmem())
        })?;

        Ok(SGTable {
            sgt,
            _owner: self.reference(),
        })
    }

    /// Creates and returns a virtual kernel memory mapping for this object.
    pub fn vmap(&self) -> Result<VMap<T>> {
        let mut map: MaybeUninit<bindings::iosys_map> = MaybeUninit::uninit();

        // SAFETY: drm_gem_shmem_vmap is thread-safe
        to_result(unsafe { bindings::drm_gem_shmem_vmap(self.mut_shmem(), map.as_mut_ptr()) })?;

        // SAFETY: if drm_gem_shmem_vmap did not fail, map is initialized now
        let map = unsafe { map.assume_init() };

        Ok(VMap {
            map,
            owner: self.reference(),
        })
    }

    /// Set the write-combine flag for this object.
    ///
    /// Should be called before any mappings are made.
    pub fn set_wc(&mut self, map_wc: bool) {
        unsafe { (*self.mut_shmem()).map_wc = map_wc };
    }
}

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

impl<T: DriverObject> crate::private::Sealed for Object<T> {}

impl<T: DriverObject> gem::IntoGEMObject for Object<T> {
    type Driver = T::Driver;

    fn gem_obj(&self) -> &bindings::drm_gem_object {
        &self.obj.base
    }

    fn mut_gem_obj(&mut self) -> &mut bindings::drm_gem_object {
        &mut self.obj.base
    }

    fn from_gem_obj(obj: *mut bindings::drm_gem_object) -> *mut Object<T> {
        crate::container_of!(obj, Object<T>, obj) as *mut Object<T>
    }
}

impl<T: DriverObject> drv::AllocImpl for Object<T> {
    const ALLOC_OPS: drv::AllocOps = drv::AllocOps {
        gem_create_object: Some(gem_create_object::<T>),
        prime_handle_to_fd: Some(bindings::drm_gem_prime_handle_to_fd),
        prime_fd_to_handle: Some(bindings::drm_gem_prime_fd_to_handle),
        gem_prime_import: None,
        gem_prime_import_sg_table: Some(bindings::drm_gem_shmem_prime_import_sg_table),
        gem_prime_mmap: Some(bindings::drm_gem_prime_mmap),
        dumb_create: Some(bindings::drm_gem_shmem_dumb_create),
        dumb_map_offset: None,
        dumb_destroy: None,
    };
}

/// A virtual mapping for a shmem-backed GEM object in kernel address space.
pub struct VMap<T: DriverObject> {
    map: bindings::iosys_map,
    owner: gem::ObjectRef<Object<T>>,
}

impl<T: DriverObject> VMap<T> {
    /// Returns a const raw pointer to the start of the mapping.
    pub fn as_ptr(&self) -> *const core::ffi::c_void {
        // SAFETY: The shmem helpers always return non-iomem maps
        unsafe { self.map.__bindgen_anon_1.vaddr }
    }

    /// Returns a mutable raw pointer to the start of the mapping.
    pub fn as_mut_ptr(&mut self) -> *mut core::ffi::c_void {
        // SAFETY: The shmem helpers always return non-iomem maps
        unsafe { self.map.__bindgen_anon_1.vaddr }
    }

    /// Returns a byte slice view of the mapping.
    pub fn as_slice(&self) -> &[u8] {
        // SAFETY: The vmap maps valid memory up to the owner size
        unsafe { slice::from_raw_parts(self.as_ptr() as *const u8, self.owner.size()) }
    }

    /// Returns mutable a byte slice view of the mapping.
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        // SAFETY: The vmap maps valid memory up to the owner size
        unsafe { slice::from_raw_parts_mut(self.as_mut_ptr() as *mut u8, self.owner.size()) }
    }

    /// Borrows a reference to the object that owns this virtual mapping.
    pub fn owner(&self) -> &gem::ObjectRef<Object<T>> {
        &self.owner
    }
}

impl<T: DriverObject> Drop for VMap<T> {
    fn drop(&mut self) {
        // SAFETY: This function is thread-safe
        unsafe {
            bindings::drm_gem_shmem_vunmap(self.owner.mut_shmem(), &mut self.map);
        }
    }
}

/// SAFETY: `iosys_map` objects are safe to send across threads.
unsafe impl<T: DriverObject> Send for VMap<T> {}
unsafe impl<T: DriverObject> Sync for VMap<T> {}

/// A single scatter-gather entry, representing a span of pages in the device's DMA address space.
///
/// For devices not behind a standalone IOMMU, this corresponds to physical addresses.
#[repr(transparent)]
pub struct SGEntry(bindings::scatterlist);

impl SGEntry {
    /// Returns the starting DMA address of this span
    pub fn dma_address(&self) -> usize {
        (unsafe { bindings::sg_dma_address(&self.0) }) as usize
    }

    /// Returns the length of this span in bytes
    pub fn dma_len(&self) -> usize {
        (unsafe { bindings::sg_dma_len(&self.0) }) as usize
    }
}

/// A scatter-gather table of DMA address spans for a GEM shmem object.
///
/// # Invariants
/// `sgt` must be a valid pointer to the `sg_table`, which must correspond to the owned
/// object in `_owner` (which ensures it remains valid).
pub struct SGTable<T: DriverObject> {
    sgt: *const bindings::sg_table,
    _owner: gem::ObjectRef<Object<T>>,
}

impl<T: DriverObject> SGTable<T> {
    /// Returns an iterator through the SGTable's entries
    pub fn iter(&'_ self) -> SGTableIter<'_> {
        SGTableIter {
            left: unsafe { (*self.sgt).nents } as usize,
            sg: unsafe { (*self.sgt).sgl },
            _p: PhantomData,
        }
    }
}

impl<'a, T: DriverObject> IntoIterator for &'a SGTable<T> {
    type Item = &'a SGEntry;
    type IntoIter = SGTableIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// SAFETY: `sg_table` objects are safe to send across threads.
unsafe impl<T: DriverObject> Send for SGTable<T> {}
unsafe impl<T: DriverObject> Sync for SGTable<T> {}

/// An iterator through `SGTable` entries.
///
/// # Invariants
/// `sg` must be a valid pointer to the scatterlist, which must outlive our lifetime.
pub struct SGTableIter<'a> {
    sg: *mut bindings::scatterlist,
    left: usize,
    _p: PhantomData<&'a ()>,
}

impl<'a> Iterator for SGTableIter<'a> {
    type Item = &'a SGEntry;

    fn next(&mut self) -> Option<Self::Item> {
        if self.left == 0 {
            None
        } else {
            let sg = self.sg;
            self.sg = unsafe { bindings::sg_next(self.sg) };
            self.left -= 1;
            Some(unsafe { &(*(sg as *const SGEntry)) })
        }
    }
}
