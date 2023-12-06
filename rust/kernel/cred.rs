// SPDX-License-Identifier: GPL-2.0

//! Credentials management.
//!
//! C header: [`include/linux/cred.h`](../../../../include/linux/cred.h)
//!
//! Reference: <https://www.kernel.org/doc/html/latest/security/credentials.html>

use crate::{
    bindings,
    task::Kuid,
    types::{AlwaysRefCounted, Opaque},
};

/// Wraps the kernel's `struct cred`.
///
/// # Invariants
///
/// Instances of this type are always ref-counted, that is, a call to `get_cred` ensures that the
/// allocation remains valid at least until the matching call to `put_cred`.
#[repr(transparent)]
pub struct Credential(pub(crate) Opaque<bindings::cred>);

// SAFETY: By design, the only way to access a `Credential` is via an immutable reference or an
// `ARef`. This means that the only situation in which a `Credential` can be accessed mutably is
// when the refcount drops to zero and the destructor runs. It is safe for that to happen on any
// thread, so it is ok for this type to be `Send`.
unsafe impl Send for Credential {}

// SAFETY: It's OK to access `Credential` through shared references from other threads because
// we're either accessing properties that don't change or that are properly synchronised by C code.
unsafe impl Sync for Credential {}

impl Credential {
    /// Creates a reference to a [`Credential`] from a valid pointer.
    ///
    /// # Safety
    ///
    /// The caller must ensure that `ptr` is valid and remains valid for the lifetime of the
    /// returned [`Credential`] reference.
    pub unsafe fn from_ptr<'a>(ptr: *const bindings::cred) -> &'a Credential {
        // SAFETY: The safety requirements guarantee the validity of the dereference, while the
        // `Credential` type being transparent makes the cast ok.
        unsafe { &*ptr.cast() }
    }

    /// Get the id for this security context.
    pub fn get_secid(&self) -> u32 {
        let mut secid = 0;
        // SAFETY: The invariants of this type ensures that the pointer is valid.
        unsafe { bindings::security_cred_getsecid(self.0.get(), &mut secid) };
        secid
    }

    /// Returns the effective UID of the given credential.
    pub fn euid(&self) -> Kuid {
        // SAFETY: By the type invariant, we know that `self.0` is valid.
        Kuid::from_raw(unsafe { (*self.0.get()).euid })
    }
}

// SAFETY: The type invariants guarantee that `Credential` is always ref-counted.
unsafe impl AlwaysRefCounted for Credential {
    fn inc_ref(&self) {
        // SAFETY: The existence of a shared reference means that the refcount is nonzero.
        unsafe { bindings::get_cred(self.0.get()) };
    }

    unsafe fn dec_ref(obj: core::ptr::NonNull<Self>) {
        // SAFETY: The safety requirements guarantee that the refcount is nonzero.
        unsafe { bindings::put_cred(obj.cast().as_ptr()) };
    }
}
