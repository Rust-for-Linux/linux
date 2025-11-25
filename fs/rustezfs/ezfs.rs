// SPDX-License-Identifier: GPL-2.0

//! Log-based filesystem written in Rust

mod defs;
mod dir;
mod inode;
mod sb;
use crate::dir::DirEntryStore;
use crate::inode::{EzfsInode, InodeStore};
use crate::sb::{EzfsSuperblock, EzfsSuperblockDisk};
use defs::*;
use kernel::dentry;
use kernel::fs::{FileSystem, Registration};
use kernel::inode::{INode, INodeState, Mapper, Params, Type};
use kernel::prelude::*;
use kernel::sb::{New, SuperBlock, Type as SuperType};
use kernel::time::UNIX_EPOCH;
use kernel::transmute::FromBytes;
use kernel::types::{ARef, Locked};
use kernel::{c_str, fs, str::CStr};

use core::marker::{PhantomData, Send, Sync};
use core::mem::size_of;
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

        let h = &*sb.data();
        let inode_store = {
            let offset = EZFS_INODE_STORE_DATABLOCK_NUMBER * EZFS_BLOCK_SIZE;
            let mapped_inode_store = h.mapper.mapped_folio(offset.try_into()?)?;
            InodeStore::from_bytes_copy(&mapped_inode_store[..size_of::<InodeStore>()])
                .ok_or(EIO)?
        };

        let ezfs_inode = inode_store[ino];
        let mode = ezfs_inode.mode();

        const DIR_IOPS: kernel::inode::Ops<RustEzFs> = kernel::inode::Ops::new::<RustEzFs>();

        let typ = match mode & fs::mode::S_IFMT {
            fs::mode::S_IFREG => {
                // inode
                //     .set_fops(file::Ops::generic_ro_file())
                //     .set_aops(FILE_AOPS);
                Type::Reg
            }
            fs::mode::S_IFDIR => {
                inode.set_iops(DIR_IOPS);
                // inode.set_iops(DIR_IOPS).set_fops(DIR_FOPS);
                Type::Dir
            }
            _ => return Err(ENOENT),
        };

        inode.init(Params {
            typ,
            mode: ezfs_inode.mode().try_into()?,
            size: ezfs_inode.file_size().try_into()?,
            blocks: ezfs_inode.nblocks(),
            nlink: ezfs_inode.nlink(),
            uid: ezfs_inode.uid(),
            gid: ezfs_inode.gid(),
            ctime: ezfs_inode.ctime()?,
            mtime: ezfs_inode.mtime()?,
            atime: ezfs_inode.atime()?,
            value: ezfs_inode,
        })
    }
}

impl FileSystem for RustEzFs {
    type Data = Pin<KBox<EzfsSuperblock>>;
    type INodeData = EzfsInode;
    const NAME: &'static CStr = c_str!("rustezfs");
    const SUPER_TYPE: SuperType = SuperType::BlockDev;

    fn fill_super(
        sb: &mut SuperBlock<Self, New>,
        mapper: Option<Mapper<Self>>,
    ) -> Result<Self::Data> {
        let Some(mapper) = mapper else {
            return Err(EINVAL);
        };

        let disk_sb = {
            let offset = EZFS_SUPERBLOCK_DATABLOCK_NUMBER * EZFS_BLOCK_SIZE;
            let mapped_sb = mapper.mapped_folio(offset.try_into()?)?;
            EzfsSuperblockDisk::from_bytes_copy(&mapped_sb).ok_or(EIO)?
        };

        if disk_sb.magic() != EZFS_MAGIC_NUMBER.try_into()? {
            return Err(EINVAL);
        }

        let ezfs_sb = KBox::pin_init(EzfsSuperblock::new(disk_sb, mapper), GFP_KERNEL)?;

        sb.set_magic(EZFS_MAGIC_NUMBER);

        Ok(ezfs_sb)
    }

    fn init_root(sb: &SuperBlock<Self>) -> Result<dentry::Root<Self>> {
        let inode = Self::iget(sb, EZFS_ROOT_INODE_NUMBER)?;
        dentry::Root::try_new(inode)
    }
}

#[vtable]
impl kernel::inode::Operations for RustEzFs {
    type FileSystem = Self;

    fn lookup(
        parent: &Locked<&INode<Self::FileSystem>, kernel::inode::ReadSem>,
        dentry: dentry::Unhashed<'_, Self::FileSystem>,
    ) -> Result<Option<ARef<dentry::DEntry<Self::FileSystem>>>> {
        let sb = &*parent.super_block();
        let h = sb.data();
        let name = dentry.name();
        let ezfs_dir_inode = parent.data();

        let dir_entries = {
            let offset = ezfs_dir_inode.data_blk_num() * EZFS_BLOCK_SIZE as u64;
            let mapped = h.mapper.mapped_folio(offset.try_into()?)?;
            DirEntryStore::from_bytes_copy(&mapped[..size_of::<DirEntryStore>()]).ok_or(EIO)?
        };

        let dir_entry = dir_entries
            .iter()
            .find(|x| x.filename() == name && x.is_active());

        let inode = if let Some(entry) = dir_entry {
            Some(Self::iget(sb, entry.inode_no().try_into()?)?)
        } else {
            None
        };

        dentry.splice_alias(inode)
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
