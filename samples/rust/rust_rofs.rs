// SPDX-License-Identifier: GPL-2.0

//! Rust read-only file system sample.

use kernel::fs::{
    DirEmitter, INode, INodeParams, INodeType, NewSuperBlock, SuperBlock, SuperParams,
};
use kernel::prelude::*;
use kernel::{c_str, folio::LockedFolio, fs, time::UNIX_EPOCH, types::ARef, types::Either};

kernel::module_fs! {
    type: RoFs,
    name: "rust_rofs",
    author: "Rust for Linux Contributors",
    description: "Rust read-only file system sample",
    license: "GPL",
}

struct Entry {
    name: &'static [u8],
    ino: u64,
    etype: INodeType,
    contents: &'static [u8],
}

const ENTRIES: [Entry; 4] = [
    Entry {
        name: b".",
        ino: 1,
        etype: INodeType::Dir,
        contents: b"",
    },
    Entry {
        name: b"..",
        ino: 1,
        etype: INodeType::Dir,
        contents: b"",
    },
    Entry {
        name: b"test.txt",
        ino: 2,
        etype: INodeType::Reg,
        contents: b"hello\n",
    },
    Entry {
        name: b"link.txt",
        ino: 3,
        etype: INodeType::Lnk,
        contents: b"./test.txt",
    },
];

struct RoFs;
impl fs::FileSystem for RoFs {
    type Data = ();
    type INodeData = &'static Entry;
    const NAME: &'static CStr = c_str!("rust-fs");

    fn super_params(_sb: &NewSuperBlock<Self>) -> Result<SuperParams<Self::Data>> {
        Ok(SuperParams {
            magic: 0x52555354,
            blocksize_bits: 12,
            maxbytes: fs::MAX_LFS_FILESIZE,
            time_gran: 1,
            data: (),
        })
    }

    fn init_root(sb: &SuperBlock<Self>) -> Result<ARef<INode<Self>>> {
        match sb.get_or_create_inode(1)? {
            Either::Left(existing) => Ok(existing),
            Either::Right(new) => new.init(INodeParams {
                typ: INodeType::Dir,
                mode: 0o555,
                size: ENTRIES.len().try_into()?,
                blocks: 1,
                nlink: 2,
                uid: 0,
                gid: 0,
                atime: UNIX_EPOCH,
                ctime: UNIX_EPOCH,
                mtime: UNIX_EPOCH,
                value: &ENTRIES[0],
            }),
        }
    }

    fn read_dir(inode: &INode<Self>, emitter: &mut DirEmitter) -> Result {
        if inode.ino() != 1 {
            return Ok(());
        }

        let pos = emitter.pos();
        if pos >= ENTRIES.len().try_into()? {
            return Ok(());
        }

        for e in ENTRIES.iter().skip(pos.try_into()?) {
            if !emitter.emit(1, e.name, e.ino, e.etype.into()) {
                break;
            }
        }

        Ok(())
    }

    fn lookup(parent: &INode<Self>, name: &[u8]) -> Result<ARef<INode<Self>>> {
        if parent.ino() != 1 {
            return Err(ENOENT);
        }

        for e in &ENTRIES {
            if name == e.name {
                return match parent.super_block().get_or_create_inode(e.ino)? {
                    Either::Left(existing) => Ok(existing),
                    Either::Right(new) => new.init(INodeParams {
                        typ: e.etype,
                        mode: 0o444,
                        size: e.contents.len().try_into()?,
                        blocks: 1,
                        nlink: 1,
                        uid: 0,
                        gid: 0,
                        atime: UNIX_EPOCH,
                        ctime: UNIX_EPOCH,
                        mtime: UNIX_EPOCH,
                        value: e,
                    }),
                };
            }
        }

        Err(ENOENT)
    }

    fn read_folio(inode: &INode<Self>, mut folio: LockedFolio<'_>) -> Result {
        let data = inode.data().contents;

        let pos = usize::try_from(folio.pos()).unwrap_or(usize::MAX);
        let copied = if pos >= data.len() {
            0
        } else {
            let to_copy = core::cmp::min(data.len() - pos, folio.size());
            folio.write(0, &data[pos..][..to_copy])?;
            to_copy
        };

        folio.zero_out(copied, folio.size() - copied)?;
        folio.mark_uptodate();
        folio.flush_dcache();

        Ok(())
    }
}
