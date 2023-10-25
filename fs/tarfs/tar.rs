// SPDX-License-Identifier: GPL-2.0

//! File system based on tar files and an index.

use core::mem::size_of;
use defs::*;
use kernel::fs::{
    DirEmitter, DirEntryType, INode, INodeParams, INodeType, NewSuperBlock, Stat, Super,
    SuperBlock, SuperParams,
};
use kernel::types::{ARef, Either, FromBytes};
use kernel::{c_str, folio::Folio, folio::LockedFolio, fs, prelude::*};

pub mod defs;

kernel::module_fs! {
    type: TarFs,
    name: "tarfs",
    author: "Wedson Almeida Filho <walmeida@microsoft.com>",
    description: "File system for indexed tar files",
    license: "GPL",
}

const SECTOR_SIZE: u64 = 512;
const TARFS_BSIZE: u64 = 1 << TARFS_BSIZE_BITS;
const TARFS_BSIZE_BITS: u8 = 12;
const SECTORS_PER_BLOCK: u64 = TARFS_BSIZE / SECTOR_SIZE;
const TARFS_MAGIC: u32 = 0x54415246;

static_assert!(SECTORS_PER_BLOCK > 0);

struct INodeData {
    offset: u64,
    flags: u8,
}

struct TarFs {
    data_size: u64,
    inode_table_offset: u64,
    inode_count: u64,
}

impl TarFs {
    fn iget(sb: &SuperBlock<Self>, ino: u64) -> Result<ARef<INode<Self>>> {
        // Check that the inode number is valid.
        let h = sb.data();
        if ino == 0 || ino > h.inode_count {
            return Err(ENOENT);
        }

        // Create an inode or find an existing (cached) one.
        let inode = match sb.get_or_create_inode(ino)? {
            Either::Left(existing) => return Ok(existing),
            Either::Right(new) => new,
        };

        static_assert!((TARFS_BSIZE as usize) % size_of::<Inode>() == 0);

        // Load inode details from storage.
        let offset = h.inode_table_offset + (ino - 1) * u64::try_from(size_of::<Inode>())?;

        let bh = sb.bread(offset / TARFS_BSIZE)?;
        let b = bh.data();
        let idata = Inode::from_bytes(b, (offset & (TARFS_BSIZE - 1)) as usize).ok_or(EIO)?;

        let mode = idata.mode.value();

        // Ignore inodes that have unknown mode bits.
        if (u32::from(mode) & !(fs::mode::S_IFMT | 0o777)) != 0 {
            return Err(ENOENT);
        }

        let doffset = idata.offset.value();
        let size = idata.size.value().try_into()?;
        let secs = u64::from(idata.lmtime.value()) | (u64::from(idata.hmtime & 0xf) << 32);
        let ts = kernel::time::Timespec::new(secs, 0)?;
        let typ = match u32::from(mode) & fs::mode::S_IFMT {
            fs::mode::S_IFREG => INodeType::Reg,
            fs::mode::S_IFDIR => INodeType::Dir,
            fs::mode::S_IFLNK => INodeType::Lnk,
            fs::mode::S_IFSOCK => INodeType::Sock,
            fs::mode::S_IFIFO => INodeType::Fifo,
            fs::mode::S_IFCHR => INodeType::Chr((doffset >> 32) as u32, doffset as u32),
            fs::mode::S_IFBLK => INodeType::Blk((doffset >> 32) as u32, doffset as u32),
            _ => return Err(ENOENT),
        };
        inode.init(INodeParams {
            typ,
            mode: mode & 0o777,
            size,
            blocks: (idata.size.value() + TARFS_BSIZE - 1) / TARFS_BSIZE,
            nlink: 1,
            uid: idata.owner.value(),
            gid: idata.group.value(),
            ctime: ts,
            mtime: ts,
            atime: ts,
            value: INodeData {
                offset: doffset,
                flags: idata.flags,
            },
        })
    }

    fn name_eq(sb: &SuperBlock<Self>, mut name: &[u8], offset: u64) -> Result<bool> {
        for v in sb.read(offset, name.len().try_into()?)? {
            let v = v?;
            let b = v.data();
            if b != &name[..b.len()] {
                return Ok(false);
            }
            name = &name[b.len()..];
        }
        Ok(true)
    }

    fn read_name(sb: &SuperBlock<Self>, mut name: &mut [u8], offset: u64) -> Result<bool> {
        for v in sb.read(offset, name.len().try_into()?)? {
            let v = v?;
            let b = v.data();
            name[..b.len()].copy_from_slice(b);
            name = &mut name[b.len()..];
        }
        Ok(true)
    }
}

impl fs::FileSystem for TarFs {
    type Data = Box<Self>;
    type INodeData = INodeData;
    const NAME: &'static CStr = c_str!("tar");
    const SUPER_TYPE: Super = Super::BlockDev;

    fn super_params(sb: &NewSuperBlock<Self>) -> Result<SuperParams<Self::Data>> {
        let scount = sb.sector_count()?;
        if scount < SECTORS_PER_BLOCK {
            pr_err!("Block device is too small: sector count={scount}\n");
            return Err(ENXIO);
        }

        let tarfs = {
            let mut folio = Folio::try_new(0)?;
            sb.sread(
                (scount / SECTORS_PER_BLOCK - 1) * SECTORS_PER_BLOCK,
                SECTORS_PER_BLOCK as usize,
                &mut folio,
            )?;
            let mapped = folio.map_page(0)?;
            let hdr =
                Header::from_bytes(&mapped, (TARFS_BSIZE - SECTOR_SIZE) as usize).ok_or(EIO)?;
            Box::try_new(TarFs {
                inode_table_offset: hdr.inode_table_offset.value(),
                inode_count: hdr.inode_count.value(),
                data_size: scount.checked_mul(SECTOR_SIZE).ok_or(ERANGE)?,
            })?
        };

        // Check that the inode table starts within the device data and is aligned to the block
        // size.
        if tarfs.inode_table_offset >= tarfs.data_size {
            pr_err!(
                "inode table offset beyond data size: {} >= {}\n",
                tarfs.inode_table_offset,
                tarfs.data_size
            );
            return Err(E2BIG);
        }

        if tarfs.inode_table_offset % SECTOR_SIZE != 0 {
            pr_err!(
                "inode table offset not aligned to sector size: {}\n",
                tarfs.inode_table_offset,
            );
            return Err(EDOM);
        }

        // Check that the last inode is within bounds (and that there is no overflow when
        // calculating its offset).
        let offset = tarfs
            .inode_count
            .checked_mul(u64::try_from(size_of::<Inode>())?)
            .ok_or(ERANGE)?
            .checked_add(tarfs.inode_table_offset)
            .ok_or(ERANGE)?;
        if offset > tarfs.data_size {
            pr_err!(
                "inode table extends beyond the data size : {} > {}\n",
                tarfs.inode_table_offset + (tarfs.inode_count * size_of::<Inode>() as u64),
                tarfs.data_size,
            );
            return Err(E2BIG);
        }

        Ok(SuperParams {
            magic: TARFS_MAGIC,
            blocksize_bits: TARFS_BSIZE_BITS,
            maxbytes: fs::MAX_LFS_FILESIZE,
            time_gran: 1000000000,
            data: tarfs,
        })
    }

    fn init_root(sb: &SuperBlock<Self>) -> Result<ARef<INode<Self>>> {
        Self::iget(sb, 1)
    }

    fn read_dir(inode: &INode<Self>, emitter: &mut DirEmitter) -> Result {
        let sb = inode.super_block();
        let mut name = Vec::<u8>::new();
        let pos = emitter.pos();

        if pos < 0 || pos % size_of::<DirEntry>() as i64 != 0 {
            return Err(ENOENT);
        }

        if pos >= inode.size() {
            return Ok(());
        }

        // Make sure the inode data doesn't overflow the data area.
        let size = u64::try_from(inode.size())?;
        if inode.data().offset.checked_add(size).ok_or(EIO)? > sb.data().data_size {
            return Err(EIO);
        }

        for v in sb.read(inode.data().offset + pos as u64, size - pos as u64)? {
            for e in DirEntry::from_bytes_to_slice(v?.data()).ok_or(EIO)? {
                let name_len = usize::try_from(e.name_len.value())?;
                if name_len > name.len() {
                    name.try_resize(name_len, 0)?;
                }

                Self::read_name(sb, &mut name[..name_len], e.name_offset.value())?;

                if !emitter.emit(
                    size_of::<DirEntry>() as i64,
                    &name[..name_len],
                    e.ino.value(),
                    DirEntryType::try_from(u32::from(e.etype))?,
                ) {
                    return Ok(());
                }
            }
        }

        Ok(())
    }

    fn lookup(parent: &INode<Self>, name: &[u8]) -> Result<ARef<INode<Self>>> {
        let name_len = u64::try_from(name.len())?;
        let sb = parent.super_block();

        for v in sb.read(parent.data().offset, parent.size().try_into()?)? {
            for e in DirEntry::from_bytes_to_slice(v?.data()).ok_or(EIO)? {
                if e.name_len.value() != name_len || e.name_len.value() > usize::MAX as u64 {
                    continue;
                }
                if Self::name_eq(sb, name, e.name_offset.value())? {
                    return Self::iget(sb, e.ino.value());
                }
            }
        }

        Err(ENOENT)
    }

    fn read_folio(inode: &INode<Self>, mut folio: LockedFolio<'_>) -> Result {
        let pos = u64::try_from(folio.pos()).unwrap_or(u64::MAX);
        let size = u64::try_from(inode.size())?;
        let sb = inode.super_block();

        let copied = if pos >= size {
            0
        } else {
            let offset = inode.data().offset.checked_add(pos).ok_or(ERANGE)?;
            let len = core::cmp::min(size - pos, folio.size().try_into()?);
            let mut foffset = 0;

            if offset.checked_add(len).ok_or(ERANGE)? > sb.data().data_size {
                return Err(EIO);
            }

            for v in sb.read(offset, len)? {
                let v = v?;
                folio.write(foffset, v.data())?;
                foffset += v.data().len();
            }
            foffset
        };

        folio.zero_out(copied, folio.size() - copied)?;
        folio.mark_uptodate();
        folio.flush_dcache();

        Ok(())
    }

    fn read_xattr(inode: &INode<Self>, name: &CStr, outbuf: &mut [u8]) -> Result<usize> {
        if inode.data().flags & inode_flags::OPAQUE == 0
            || name.as_bytes() != b"trusted.overlay.opaque"
        {
            return Err(ENODATA);
        }

        if !outbuf.is_empty() {
            outbuf[0] = b'y';
        }

        Ok(1)
    }

    fn statfs(sb: &SuperBlock<Self>) -> Result<Stat> {
        let data = sb.data();
        Ok(Stat {
            magic: TARFS_MAGIC,
            namelen: i64::MAX,
            bsize: TARFS_BSIZE as _,
            blocks: data.inode_table_offset / TARFS_BSIZE,
            files: data.inode_count,
        })
    }
}
