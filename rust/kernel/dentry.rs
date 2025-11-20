use core::{marker::PhantomData, mem::ManuallyDrop, ops::Deref, ptr};

use crate::{
    bindings,
    error::Result,
    fs::FileSystem,
    inode::INode,
    prelude::*,
    sb::SuperBlock,
    types::{ARef, AlwaysRefCounted, Opaque},
};

#[repr(transparent)]
pub struct DEntry<T: FileSystem + ?Sized>(pub(crate) Opaque<bindings::dentry>, PhantomData<T>);

// SAFETY: The type invariants guarantee that `DEntry` is always ref-counted.
unsafe impl<T: FileSystem + ?Sized> AlwaysRefCounted for DEntry<T> {
    fn inc_ref(&self) {
        // SAFETY: The existence of a shared reference means that the refcount is nonzero.
        unsafe { bindings::dget(self.0.get()) };
    }

    unsafe fn dec_ref(obj: ptr::NonNull<Self>) {
        // SAFETY: The safety requirements guarantee that the refcount is nonzero.
        unsafe { bindings::dput(obj.as_ref().0.get()) }
    }
}

impl<T: FileSystem + ?Sized> DEntry<T> {
    /// Creates a new [`DEntry`] from a raw C pointer.
    ///
    /// # Safety
    ///
    /// * `ptr` must be valid for at least the lifetime of the returned reference.
    /// * `ptr` has the correct file system type, or `T` is [`super::UnspecifiedFS`].
    pub(crate) unsafe fn from_raw<'a>(ptr: *mut bindings::dentry) -> &'a Self {
        // SAFETY: The safety requirements guarantee that the reference is and remains valid.
        unsafe { &*ptr.cast::<Self>() }
    }

    /// Returns the superblock of the dentry.
    pub fn super_block(&self) -> &SuperBlock<T> {
        // `d_sb` is immutable, so it's safe to read it.
        unsafe { SuperBlock::from_raw((*self.0.get()).d_sb) }
    }
}

pub struct Root<T: FileSystem + ?Sized>(ARef<DEntry<T>>);

impl<T: FileSystem + ?Sized> Root<T> {
    /// Creates a root dentry.
    pub fn try_new(inode: ARef<INode<T>>) -> Result<Root<T>> {
        // SAFETY: `d_make_root` requires that `inode` be valid and referenced, which is the
        // case for this call.
        //
        // It takes over the inode, even on failure, so we don't need to clean it up.
        let dentry_ptr = unsafe { bindings::d_make_root(ManuallyDrop::new(inode).0.get()) };
        let dentry = ptr::NonNull::new(dentry_ptr).ok_or(ENOMEM)?;

        // SAFETY: `dentry` is valid and referenced. It reference ownership is transferred to
        // `ARef`.
        Ok(Root(unsafe { ARef::from_raw(dentry.cast::<DEntry<T>>()) }))
    }
}

impl<T: FileSystem + ?Sized> Deref for Root<T> {
    type Target = DEntry<T>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
