// SPDX-License-Identifier: GPL-2.0

//! Log-based filesystem written in Rust

mod defs;
mod dir;
mod inode;
mod sb;
use crate::dir::{DirEntryStore, EzfsDirEntry};
use crate::inode::{EzfsInode, InodeStore};
use crate::sb::{EzfsSuperblock, EzfsSuperblockDisk};
use defs::*;
use kernel::dentry;
use kernel::fs::{file, File, FileSystem, Offset, Registration};
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
    const DIR_FOPS: file::Ops<RustEzFs> = file::Ops::new::<RustEzFs>();
    const DIR_IOPS: kernel::inode::Ops<RustEzFs> = kernel::inode::Ops::new::<RustEzFs>();

    fn iget(sb: &SuperBlock<Self>, ino: usize) -> Result<ARef<INode<Self>>> {
        let mut inode = match sb.get_or_create_inode(ino)? {
            INodeState::Existing(inode) => return Ok(inode),
            INodeState::Uninitilized(new_inode) => new_inode,
        };

        let h = &*sb.data();

        let offset = EZFS_INODE_STORE_DATABLOCK_NUMBER * EZFS_BLOCK_SIZE;
        let mapped_inode_store = h.mapper.mapped_folio(offset.try_into()?)?;
        let inode_store =
            InodeStore::from_bytes(&mapped_inode_store[..size_of::<InodeStore>()]).ok_or(EIO)?;

        let ezfs_inode = inode_store[ino - 1];
        let mode = ezfs_inode.mode();

        let typ = match mode & fs::mode::S_IFMT {
            fs::mode::S_IFREG => {
                inode
                    .set_iops(Self::DIR_IOPS)
                    .set_fops(file::Ops::generic_ro_file());
                // .set_aops(FILE_AOPS);
                Type::Reg
            }
            fs::mode::S_IFDIR => {
                inode.set_iops(Self::DIR_IOPS).set_fops(Self::DIR_FOPS);
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
        pr_info!("looking for file: {:?}", core::str::from_utf8(name));

        if name.len() > EZFS_FILENAME_BUF_SIZE {
            pr_info!("dentry name to long: {:?}", core::str::from_utf8(name));
            return Err(ENAMETOOLONG);
        }

        let ezfs_dir_inode = parent.data();
        pr_info!("ezfs_dir inode number: {:?}", parent.ino());
        pr_info!("ezfs dir inode links: {:?}", ezfs_dir_inode.nlink());
        pr_info!("data_blk_num: {:?}", ezfs_dir_inode.data_blk_num());

        let offset = ezfs_dir_inode
            .data_blk_num()
            .checked_mul(EZFS_BLOCK_SIZE as u64)
            .ok_or(EIO)?;

        let mapped = h.mapper.mapped_folio(offset.try_into()?)?;
        let dir_entries =
            DirEntryStore::from_bytes(&mapped[..size_of::<DirEntryStore>()]).ok_or(EIO)?;

        let dir_entry = dir_entries.iter().find(|x| {
            pr_info!(
                "filename: {:?} = {}\n",
                x.filename(),
                core::str::from_utf8(x.filename()).unwrap_or("<invalid utf8>")
            );

            pr_info!(
                "dname: {:?} = {}\n",
                name,
                core::str::from_utf8(name).unwrap_or("<invalid utf8>")
            );
            x.filename() == name && x.is_active()
        });

        let inode = if let Some(entry) = dir_entry {
            pr_info!("Inode found: {:?}", entry.inode_no());
            Some(Self::iget(sb, entry.inode_no().try_into()?)?)
        } else {
            None
        };

        dentry.splice_alias(inode)
    }
}

#[vtable]
impl file::Operations for RustEzFs {
    type FileSystem = Self;

    fn seek(file: &File<Self>, offset: Offset, whence: file::Whence) -> Result<Offset> {
        file::generic_seek(file, offset, whence)
    }

    fn read(_: &File<Self>, _: &mut kernel::user::Writer, _: &mut Offset) -> Result<usize> {
        Err(EISDIR)
    }

    fn read_dir(
        file: &File<Self>,
        inode: &Locked<&INode<Self>, kernel::inode::ReadSem>,
        emitter: &mut file::DirEmitter,
    ) -> Result {
        let pos: usize = emitter.pos().try_into().map_err(|_| ENOENT)?;
        pr_info!("emitter position: {:?}", pos);

        if pos < 2 {
            pr_info!("pos < 2: trying to emit dots");
            pr_info!("file inode: {:?}", file.inode().ino());
            if !emitter.emit_dots(file) {
                return Ok(());
            }
        }

        let sb = &*inode.super_block();
        let h = sb.data();

        let index = {
            let disk_pos = pos.checked_sub(2).ok_or(ENOENT)?;
            pr_info!("disk position: {:?}", disk_pos);

            if disk_pos % size_of::<EzfsDirEntry>() != 0 {
                return Err(ENOENT);
            }

            disk_pos / size_of::<EzfsDirEntry>()
        };

        pr_info!("emitter index: {:?}", index);

        if index >= EZFS_MAX_CHILDREN {
            pr_info!("index higher than max children: {:?}", index);
            return Ok(());
        }

        let ezfs_dir_inode = inode.data();
        pr_info!("inode data_blk_num: {:?}", ezfs_dir_inode.data_blk_num());

        let offset = ezfs_dir_inode
            .data_blk_num()
            .checked_mul(EZFS_BLOCK_SIZE as u64)
            .ok_or(EIO)?;

        pr_info!("valid offset: {:?}", offset);

        let mapped = h.mapper.mapped_folio(offset.try_into()?)?;
        let dir_entries =
            DirEntryStore::from_bytes(&mapped[..size_of::<DirEntryStore>()]).ok_or(EIO)?;
        pr_info!("found dir_entries");

        let inode_store_offset = EZFS_INODE_STORE_DATABLOCK_NUMBER * EZFS_BLOCK_SIZE;
        let mapped_inode_store = h.mapper.mapped_folio(inode_store_offset.try_into()?)?;
        let inode_store =
            InodeStore::from_bytes(&mapped_inode_store[..size_of::<InodeStore>()]).ok_or(EIO)?;

        let active_entries = dir_entries
            .iter()
            .skip(index)
            .filter(|&entry| entry.is_active());

        for entry in active_entries {
            let ino: usize = entry.inode_no().try_into()?;
            let entry_inode = inode_store[ino - EZFS_ROOT_INODE_NUMBER];
            let etype = file::DirEntryType::from_mode(entry_inode.mode());

            if !emitter.emit(
                size_of::<EzfsDirEntry>() as i64,
                entry.filename(),
                entry.inode_no(),
                etype,
            ) {
                return Ok(());
            }
        }

        Ok(())
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
