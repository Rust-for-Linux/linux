use crate::defs::*;
use core::ops::Deref;
use kernel::error::Result;
use kernel::time::Timespec;
use kernel::transmute::FromBytes;
use kernel::uapi::{gid_t, mode_t, uid_t};

#[repr(C)]
#[derive(Copy, Clone)]
pub(crate) struct EzfsInode {
    mode: mode_t,
    uid: uid_t,
    gid: gid_t,
    i_atime: i64, /* access time */
    i_mtime: i64, /* modified time */
    i_ctime: i64, /* change time */
    nlink: u32,
    data_blk_num: u64,
    file_size: u64,
    nblocks: u64,
}

impl EzfsInode {
    pub(crate) fn mode(&self) -> mode_t {
        self.mode
    }

    pub(crate) fn uid(&self) -> uid_t {
        self.uid
    }

    pub(crate) fn gid(&self) -> gid_t {
        self.gid
    }

    pub(crate) fn atime(&self) -> Result<Timespec> {
        Timespec::new(self.i_atime.try_into()?, 0)
    }

    pub(crate) fn mtime(&self) -> Result<Timespec> {
        Timespec::new(self.i_mtime.try_into()?, 0)
    }

    pub(crate) fn ctime(&self) -> Result<Timespec> {
        Timespec::new(self.i_ctime.try_into()?, 0)
    }

    pub(crate) fn nlink(&self) -> u32 {
        self.nlink
    }

    pub(crate) fn data_blk_num(&self) -> u64 {
        self.data_blk_num
    }

    pub(crate) fn file_size(&self) -> u64 {
        self.file_size
    }

    pub(crate) fn nblocks(&self) -> u64 {
        self.nblocks
    }
}

#[repr(C)]
pub(crate) struct InodeStore {
    inodes: [EzfsInode; EZFS_MAX_INODES],
}

impl Deref for InodeStore {
    type Target = [EzfsInode];

    fn deref(&self) -> &Self::Target {
        &self.inodes
    }
}

// SAFETY: EzfsInode is FromBytes, so array of them is too
unsafe impl FromBytes for InodeStore {}
