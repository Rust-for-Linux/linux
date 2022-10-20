// SPDX-License-Identifier: GPL-2.0

use kernel::{
    bindings, new_mutex,
    prelude::*,
    security,
    sync::{Arc, Mutex},
};

use crate::{
    node::NodeRef,
    thread::{BinderError, BinderResult},
};

struct Manager {
    node: Option<NodeRef>,
    uid: Option<bindings::kuid_t>,
}

#[pin_project]
pub(crate) struct Context {
    #[pin]
    manager: Mutex<Manager>,
}

#[allow(clippy::non_send_fields_in_send_ty)]
unsafe impl Send for Context {}
unsafe impl Sync for Context {}

impl Context {
    pub(crate) fn new() -> Result<Arc<Self>> {
        Arc::pin_init(pin_init!(Self {
            manager: new_mutex!(
                Manager {
                    node: None,
                    uid: None,
                },
                "Contex::manager"
            ),
        }))
    }

    pub(crate) fn set_manager_node(&self, node_ref: NodeRef) -> Result {
        let mut manager = self.manager.lock();
        if manager.node.is_some() {
            return Err(EBUSY);
        }
        security::binder_set_context_mgr(&node_ref.node.owner.cred)?;

        // TODO: Get the actual caller id.
        let caller_uid = bindings::kuid_t::default();
        if let Some(ref uid) = manager.uid {
            if uid.val != caller_uid.val {
                return Err(EPERM);
            }
        }

        manager.node = Some(node_ref);
        manager.uid = Some(caller_uid);
        Ok(())
    }

    pub(crate) fn unset_manager_node(&self) {
        let node_ref = self.manager.lock().node.take();
        drop(node_ref);
    }

    pub(crate) fn get_manager_node(&self, strong: bool) -> BinderResult<NodeRef> {
        self.manager
            .lock()
            .node
            .as_ref()
            .ok_or_else(BinderError::new_dead)?
            .clone(strong)
    }
}
