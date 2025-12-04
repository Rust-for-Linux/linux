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
use kernel::iov::{IovIterDest, IovIterSource};
use kernel::prelude::*;
use kernel::sb::{New, SuperBlock, Type as SuperType};
use kernel::time::UNIX_EPOCH;
use kernel::transmute::FromBytes;
use kernel::types::{ARef, Lockable, Locked};
use kernel::{address_space, iomap};
use kernel::{c_str, fs, str::CStr};

use core::marker::{PhantomData, Send, Sync};
use core::mem::size_of;
use core::ptr;
use pin_init::{pin_data, PinInit, PinnedDrop};

struct RustEzFs;

#[pin_data]
struct RustEzFsModule<RustEzFs> {
    #[pin]
    fs_reg: Registration,
    _p: PhantomData<RustEzFs>,
}

macro_rules! min {
    ($a:expr, $b:expr) => {{
        let a_val = $a;
        let b_val = $b;
        if a_val < b_val {
            a_val
        } else {
            b_val
        }
    }};
}

fn get_max_blocks(sb: Pin<&EzfsSuperblock>) -> u64 {
    min!(sb.disk_blocks - 2, EZFS_MAX_DATA_BLKS as u64)
}

fn ezfs_move_block(mut from: u64, mut to: u64, sb: &SuperBlock<RustEzFs>) -> Result {
    from += EZFS_ROOT_DATABLOCK_NUMBER as u64;
    to += EZFS_ROOT_DATABLOCK_NUMBER as u64;

    let src = sb.read_mapping_page(from)?;
    let dst = sb.read_mapping_page(to)?;

    src.copy_to(&dst);

    Ok(())
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

    fn write_iter(
        mut kiocb: Kiocb<'_, <Self::FileSystem as FileSystem>::Data>,
        mut iov: &mut IovIterSource<'_>,
    ) -> Result<usize> {
        pr_info!("write_iter\n");
        let flags = kiocb.ki_flags();
        let file: &File<Self> = kiocb.ki_filp();

        if flags & bindings::IOCB_DIRECT != 0 {
            return Err(EINVAL); // We don't support direct I/O
        }

        if (file.flags() & bindings::O_APPEND) != 0 {
            *kiocb.ki_pos_mut() = file.host_inode().size();
        }

        // SAFETY: We've got a valid kiocb and iov iter from our VFS and our iomap_ops is static
        // The function treats null pointers as valid inputs for the last two params
        iomap::file_buffered_write::<RustEzFs>(kiocb, iov)
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

        // pr_info!("start_block: {start_block}, end_block: {end_block}\n");

        let ez_blk_num = ezfs_inode.data_blk_num();
        let ez_blk_count = inode.blocks() / 8;

        // pr_info!("blk_num: {ez_blk_num}, ez_blk_count: {ez_blk_count}\n");

        let mut phys = if ez_blk_num > 0 {
            ez_blk_num + start_block
        } else {
            0
        };

        // For all cases, the bdev, offset and length are as such
        map.set_bdev(Some(sb.bdev()))
            .set_offset(pos)
            .set_length(length as u64);

        // We're reading
        if (flags & iomap::flags::WRITE == 0) {
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

        // We're writing
        // As we'll modify the file system below, we must acquire a lock
        ezfs_sb.lock();

        let max_blocks = get_max_blocks(ezfs_sb);
        let blocks_needed = end_block + 1;
        let blocks_to_add = blocks_needed - ez_blk_count;

        // TODO: is this necessary ?
        if blocks_needed > max_blocks {
            return Err(ENOSPC);
        }

        enum WriteCase {
            NEW,    // Write to an empty file without any allocated blocks
            WITHIN, // File can fit written contents within allocated, unused block
            EXTEND, // File has adjacent, free block to extend to
            MOVE,   // File has no adjacent, free block and must be moved
        }

        let mut free_data_blocks = ezfs_sb.free_data_blocks.lock();
        let ez_blk_sidx = ez_blk_num - TryInto::<u64>::try_into(EZFS_ROOT_DATABLOCK_NUMBER)?;

        let case_type = if ez_blk_num == 0 {
            WriteCase::NEW
        } else if blocks_to_add <= 0 {
            WriteCase::WITHIN
        } else {
            let start = ez_blk_sidx + ez_blk_count;
            let end = ez_blk_sidx + blocks_needed;

            if end > max_blocks {
                return Err(ENOSPC);
            }

            // pr_info!("start={start} - end={end}\n");

            if (start..end).any(|bit| free_data_blocks.is_set(bit)) {
                WriteCase::MOVE
            } else {
                WriteCase::EXTEND
            }
        };

        match case_type {
            WriteCase::NEW => {
                pr_info!("adding to an empty file\n");
                return Err(EIO);
            }
            WriteCase::WITHIN => {}
            WriteCase::EXTEND => {
                for i in ez_blk_count..blocks_needed {
                    let bit = ez_blk_sidx + i;
                    free_data_blocks.set_bit(bit);
                }

                map.set_flags(iomap::map_flags::NEW);
            }
            WriteCase::MOVE => {
                // Let's try to find a region of sequential free blocks
                // of size `blocks_needed` to move our file to
                let mut curr_block = 0;
                let mut seen_free = 0;
                while seen_free < blocks_needed && curr_block < max_blocks {
                    // if block isn't free, we reset counter
                    if free_data_blocks.is_set(curr_block) {
                        seen_free = 0;
                    } else {
                        seen_free += 1;
                    }

                    curr_block += 1;
                }

                if (seen_free < blocks_needed) {
                    return Err(ENOSPC);
                }

                // Move all blocks within the file to new region
                let new_block_start = curr_block - blocks_needed;

                if (ez_blk_num != 0) {
                    for j in 0..ez_blk_count {
                        let old = ez_blk_sidx + j;
                        let new = new_block_start + j;

                        ezfs_move_block(old, new, sb);

                        free_data_blocks.clear_bit(old);
                        free_data_blocks.set_bit(new);
                    }
                }

                // SAFETY: we've acquired the super block lock and can therefore
                // modify the ezfs inode
                let mut ezfs_inode = unsafe { inode.data_mut() };
                ezfs_inode.data_blk_num = new_block_start + (EZFS_ROOT_DATABLOCK_NUMBER as u64);
                phys = ezfs_inode.data_blk_num() + start_block;
                map.set_flags(iomap::map_flags::NEW);
            }
        }

        map.set_type(iomap::Type::Mapped)
            .set_addr(phys << sb.blocksize_bits());

        Ok(())
    }

    fn end<'a>(
        inode: &'a INode<Self::FileSystem>,
        _pos: Offset,
        _length: Offset,
        written: isize,
        _flags: u32,
        _map: &iomap::Map<'a>,
    ) -> Result {
        if (written > 0) {
            pr_info!("iomap_end()\n");
            let new_blocks =
                ((inode.size() + (EZFS_BLOCK_SIZE as i64) - 1) / EZFS_BLOCK_SIZE as i64) as u64;
            let sb = inode.super_block();
            let ezfs_sb = sb.data();

            // We'll modify our inodes, let's lock first
            ezfs_sb.lock();

            // SAFETY: We've acquired the super block lock
            unsafe { inode.set_blocks(new_blocks * 8) };
            let ezfs_inode = unsafe { inode.data_mut() };

            ezfs_inode.nblocks = new_blocks;

            // TODO:
            // - get inode store and update nblocks
            // - mark inode dirty
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
