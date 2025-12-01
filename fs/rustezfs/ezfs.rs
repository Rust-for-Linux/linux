// SPDX-License-Identifier: GPL-2.0

//! Easy filesystem written in Rust
#![allow(unused)]

mod defs;
mod dir;
mod inode;
mod sb;
use crate::dir::{DirEntryStore, EzfsDirEntry};
use crate::inode::{EzfsInode, InodeStore};
use crate::sb::{EzfsSuperblock, EzfsSuperblockDisk};
use defs::*;
use kernel::bindings;
use kernel::dentry;
use kernel::folio::{Folio, PageCache};
use kernel::fs::Kiocb;
use kernel::fs::{file, File, FileSystem, Offset, Registration};
use kernel::inode::{INode, INodeState, Mapper, Params, Type};
use kernel::iov::IovIterDest;
use kernel::prelude::*;
use kernel::sb::{New, SuperBlock, Type as SuperType};
use kernel::time::UNIX_EPOCH;
use kernel::transmute::FromBytes;
use kernel::types::{ARef, Lockable, Locked};
use kernel::{address_space, iomap};
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
    const FILE_FOPS: file::Ops<RustEzFs> = file::Ops::new_file::<RustEzFs>();
    const DIR_FOPS: file::Ops<RustEzFs> = file::Ops::new_dir::<RustEzFs>();
    const IOPS: kernel::inode::Ops<RustEzFs> = kernel::inode::Ops::new::<RustEzFs>();
    const AOPS: kernel::address_space::Ops<RustEzFs> = kernel::iomap::aops::<RustEzFs>();

    fn iget(sb: &SuperBlock<Self>, ino: usize) -> Result<ARef<INode<Self>>> {
        pr_info!("iget(ino={ino})\n");
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
                inode.set_fops(Self::FILE_FOPS);
                Type::Reg
            }
            fs::mode::S_IFDIR => {
                inode.set_fops(Self::DIR_FOPS);
                Type::Dir
            }
            _ => return Err(ENOENT),
        };

        inode.set_iops(Self::IOPS).set_aops(Self::AOPS);

        inode.init(Params {
            typ,
            mode: ezfs_inode.mode().try_into()?,
            size: ezfs_inode.file_size().try_into()?,
            blocks: ezfs_inode.nblocks() * 8,
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
        pr_info!("fill_super()\n");
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
        pr_info!("init_root()\n");
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
        pr_info!("lookup(name={:?})\n", core::str::from_utf8(name));

        if name.len() > EZFS_FILENAME_BUF_SIZE {
            return Err(ENAMETOOLONG);
        }

        let ezfs_dir_inode = parent.data();
        // pr_info!("ezfs_dir inode number: {:?}", parent.ino());
        // pr_info!("ezfs dir inode links: {:?}", ezfs_dir_inode.nlink());
        // pr_info!("data_blk_num: {:?}", ezfs_dir_inode.data_blk_num());

        let offset = ezfs_dir_inode
            .data_blk_num()
            .checked_mul(EZFS_BLOCK_SIZE as u64)
            .ok_or(EIO)?;

        let mapped = h.mapper.mapped_folio(offset.try_into()?)?;
        let dir_entries =
            DirEntryStore::from_bytes(&mapped[..size_of::<DirEntryStore>()]).ok_or(EIO)?;

        let dir_entry = dir_entries
            .iter()
            .find(|x| x.is_active() && x.filename() == name);

        let inode = if let Some(entry) = dir_entry {
            pr_info!("Inode found: {:?}\n", entry.inode_no());
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
        pr_info!("seek()\n");
        file::generic_seek(file, offset, whence)
    }

    // TODO: file::Operations currently just calls generic_file_read_iter directly
    // we might want to move it here but that requires us being able to have both
    // Kiocb and IovIterDest implement ::to_ptr(); Let's do that later
    // fn read_iter(
    //     _kiocb: Kiocb<'_, <Self as FileSystem>::Data>,
    //     _iov: &mut IovIterDest<'_>,
    // ) -> Result<usize> {
    //     pr_info!("read_iter()\n");
    //
    //     // from_result(|| {
    //     //     let res = unsafe { bindings::generic_file_read_iter() };
    //     // })
    //
    //     Err(EINVAL)
    // }

    fn read_dir(
        file: &File<Self>,
        inode: &Locked<&INode<Self>, kernel::inode::ReadSem>,
        emitter: &mut file::DirEmitter,
    ) -> Result {
        pr_info!("read_dir()\n");
        let pos: usize = emitter.pos().try_into().map_err(|_| ENOENT)?;

        if pos < 2 {
            if !emitter.emit_dots(file) {
                return Ok(());
            }
        }

        let sb = &*inode.super_block();
        let h = sb.data();

        let index = {
            let disk_pos = pos.checked_sub(2).ok_or(ENOENT)?;

            if disk_pos % size_of::<EzfsDirEntry>() != 0 {
                return Err(ENOENT);
            }

            disk_pos / size_of::<EzfsDirEntry>()
        };

        // pr_info!("emitter index: {:?}", index);

        if index >= EZFS_MAX_CHILDREN {
            return Ok(());
        }

        let ezfs_dir_inode = inode.data();
        // pr_info!("inode data_blk_num: {:?}", ezfs_dir_inode.data_blk_num());

        let offset = ezfs_dir_inode
            .data_blk_num()
            .checked_mul(EZFS_BLOCK_SIZE as u64) // TODO: better check?
            .ok_or(EIO)?;

        let mapped = h.mapper.mapped_folio(offset.try_into()?)?;
        let dir_entries =
            DirEntryStore::from_bytes(&mapped[..size_of::<DirEntryStore>()]).ok_or(EIO)?;

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

impl iomap::Operations for RustEzFs {
    type FileSystem = Self;

    // Equivalent to c's iomap_begin()
    fn begin<'a>(
        inode: &'a INode<Self::FileSystem>,
        pos: Offset,
        length: Offset,
        flags: u32,
        map: &mut iomap::Map<'a>,
        srcmap: &mut iomap::Map<'a>,
    ) -> Result {
        pr_info!("iomap_begin()\n");

        let sb = inode.super_block();
        let ezfs_sb: Pin<&EzfsSuperblock> = sb.data();
        let ezfs_inode = inode.data();

        let start_block: u64 = (pos >> sb.blocksize_bits()).try_into()?;
        let end_block: u64 = ((pos + length - 1) >> sb.blocksize_bits()).try_into()?;

        let ez_blk_num = ezfs_inode.data_blk_num();
        let ez_blk_count = inode.blocks() / 8;

        let phys = if ez_blk_num > 0 {
            ez_blk_num + start_block
        } else {
            0
        };

        // For all cases, the bdev, offset and length are as such
        map.set_bdev(Some(sb.bdev()))
            .set_offset(pos)
            .set_length(length as u64);

        if (flags & iomap::flags::WRITE == 0) {
            pr_info!("READING\n");

            // Invalid read, block does not belong to inode
            if ez_blk_num == 0 || start_block >= ez_blk_count {
                map.set_type(iomap::Type::Hole)
                    .set_addr(bindings::IOMAP_NULL_ADDR as u64);
                return Ok(());
            }
            // Valid read, set target address accordingly
            map.set_type(iomap::Type::Mapped)
                .set_addr(phys << sb.blocksize_bits());
            return Ok(());
        };

        pr_info!("WRITING\n");

        // // Shifted physical index
        // let phys_sidx: i64 = if ez_blk_num > 0 {
        //     // phys should always be >= root datablock number
        //     (phys - EZFS_ROOT_DATABLOCK_NUMBER as u64).try_into()?
        // } else {
        //     -1i64
        // };

        Err(EIO)
    }

    fn end<'a>(
        _inode: &'a INode<Self::FileSystem>,
        _pos: Offset,
        _length: Offset,
        _written: isize,
        _flags: u32,
        _map: &iomap::Map<'a>,
    ) -> Result {
        pr_info!("iomap_end()\n");
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
