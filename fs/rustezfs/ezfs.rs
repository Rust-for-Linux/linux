// SPDX-License-Identifier: GPL-2.0

//! Log-based filesystem written in Rust

mod defs;
mod dir;
mod inode;
mod sb;
use defs::*;
use kernel::dentry;
use kernel::fs::{FileSystem, Registration};
use kernel::inode::{INode, INodeState, Mapper, Params, Type};
use kernel::prelude::*;
use kernel::sb::{New, SuperBlock};
use kernel::time::UNIX_EPOCH;
use kernel::types::ARef;
use kernel::{c_str, fs, str::CStr};

use core::marker::{PhantomData, Send, Sync};
use pin_init::{pin_data, PinInit, PinnedDrop};

struct RustEzFs;

#[pin_data]
struct RustEzFsModule<RustEzFs> {
    #[pin]
    fs_reg: Registration,
    _p: PhantomData<RustEzFs>,
}

impl kernel::InPlaceModule for RustEzFsModule<RustEzFs> {
    fn init(module: &'static ThisModule) -> impl PinInit<Self, Error> {
        try_pin_init!(Self {
            fs_reg <- Registration::new::<RustEzFs>(module),
            _p: PhantomData,
        })
    }
}

impl RustEzFs {
    fn iget(sb: &SuperBlock<Self>, ino: usize) -> Result<ARef<INode<Self>>> {
        let mut inode = match sb.get_or_create_inode(ino)? {
            INodeState::Existing(inode) => return Ok(inode),
            INodeState::Uninitilized(new_inode) => new_inode,
        };

        inode.init(Params {
            typ: Type::Dir,
            mode: 0o755,
            size: 0,
            blocks: 0,
            nlink: 2,
            uid: 0,
            gid: 0,
            ctime: UNIX_EPOCH,
            mtime: UNIX_EPOCH,
            atime: UNIX_EPOCH,
            value: (),
        })
    }
}

impl FileSystem for RustEzFs {
    // type Data = KBox<Self>;
    type Data = ();
    type INodeData = ();
    const NAME: &'static CStr = c_str!("rustezfs");

    fn fill_super(sb: &mut SuperBlock<Self, New>, _: Option<Mapper<Self>>) -> Result<Self::Data> {
        sb.set_magic(EZFS_MAGIC_NUMBER);
        Ok(())
    }

    fn init_root(sb: &SuperBlock<Self>) -> Result<dentry::Root<Self>> {
        let inode = Self::iget(sb, EZFS_ROOT_INODE_NUMBER)?;
        dentry::Root::try_new(inode)
    }
}

type FsModule = RustEzFsModule<RustEzFs>;

module! {
    type: FsModule,
    name: "rustezfs",
    authors: ["ls4121@columbia.edu", "kfb2117@columbia.edu"],
    description: "Easy file system in Rust",
    license: "GPL",
}
