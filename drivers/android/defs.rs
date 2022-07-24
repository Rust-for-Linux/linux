// SPDX-License-Identifier: GPL-2.0

use core::ops::{Deref, DerefMut};
use kernel::{
    bindings,
    bindings::*,
    io_buffer::{ReadableFromBytes, WritableToBytes},
};

macro_rules! pub_no_prefix {
    ($prefix:ident, $($newname:ident),+) => {
        $(pub(crate) const $newname: u32 = kernel::macros::concat_idents!($prefix, $newname);)+
    };
}

pub_no_prefix!(
    binder_driver_return_protocol_,
    BR_OK,
    BR_ERROR,
    BR_TRANSACTION,
    BR_REPLY,
    BR_DEAD_REPLY,
    BR_TRANSACTION_COMPLETE,
    BR_INCREFS,
    BR_ACQUIRE,
    BR_RELEASE,
    BR_DECREFS,
    BR_NOOP,
    BR_SPAWN_LOOPER,
    BR_DEAD_BINDER,
    BR_CLEAR_DEATH_NOTIFICATION_DONE,
    BR_FAILED_REPLY
);

pub_no_prefix!(
    binder_driver_command_protocol_,
    BC_TRANSACTION,
    BC_REPLY,
    BC_FREE_BUFFER,
    BC_INCREFS,
    BC_ACQUIRE,
    BC_RELEASE,
    BC_DECREFS,
    BC_INCREFS_DONE,
    BC_ACQUIRE_DONE,
    BC_REGISTER_LOOPER,
    BC_ENTER_LOOPER,
    BC_EXIT_LOOPER,
    BC_REQUEST_DEATH_NOTIFICATION,
    BC_CLEAR_DEATH_NOTIFICATION,
    BC_DEAD_BINDER_DONE
);

pub_no_prefix!(transaction_flags_, TF_ONE_WAY, TF_ACCEPT_FDS);

pub(crate) use bindings::{
    BINDER_TYPE_BINDER, BINDER_TYPE_FD, BINDER_TYPE_HANDLE, BINDER_TYPE_WEAK_BINDER,
    BINDER_TYPE_WEAK_HANDLE, FLAT_BINDER_FLAG_ACCEPTS_FDS,
};

macro_rules! decl_wrapper {
    ($newname:ident, $wrapped:ty) => {
        #[derive(Copy, Clone, Default)]
        pub(crate) struct $newname($wrapped);

        // TODO: This must be justified by inspecting the type, so should live outside the macro or
        // the macro should be somehow marked unsafe.
        unsafe impl ReadableFromBytes for $newname {}
        unsafe impl WritableToBytes for $newname {}

        impl Deref for $newname {
            type Target = $wrapped;
            fn deref(&self) -> &Self::Target {
                &self.0
            }
        }

        impl DerefMut for $newname {
            fn deref_mut(&mut self) -> &mut Self::Target {
                &mut self.0
            }
        }
    };
}

decl_wrapper!(BinderNodeDebugInfo, bindings::binder_node_debug_info);
decl_wrapper!(BinderNodeInfoForRef, bindings::binder_node_info_for_ref);
decl_wrapper!(FlatBinderObject, bindings::flat_binder_object);
decl_wrapper!(BinderTransactionData, bindings::binder_transaction_data);
decl_wrapper!(BinderWriteRead, bindings::binder_write_read);
decl_wrapper!(BinderVersion, bindings::binder_version);

impl BinderVersion {
    pub(crate) fn current() -> Self {
        Self(bindings::binder_version {
            protocol_version: bindings::BINDER_CURRENT_PROTOCOL_VERSION as _,
        })
    }
}
