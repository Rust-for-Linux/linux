use crate::{defs::*, RustEzFs};
use core::ops::{Deref, DerefMut};
use kernel::error::Result;
use kernel::inode::INode;
use kernel::time::Timespec;
use kernel::transmute::{AsBytes, FromBytes};
use kernel::uapi::{gid_t, mode_t, uid_t};

#[repr(C)]
#[derive(Copy, Clone, Default)]
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
    pub(crate) fn from_vfs_inode(vfs_inode: &INode<RustEzFs>) -> Result<Self> {
        let mut disk_inode = EzfsInode::default();

        let mtime_sec = vfs_inode.mtime()?.tv_sec();
        let ctime_sec = vfs_inode.ctime()?.tv_sec();
        let atime_sec = vfs_inode.atime()?.tv_sec();

        disk_inode.set_file_size(vfs_inode.size().try_into()?);
        disk_inode.set_nblocks(vfs_inode.blocks() / 8);
        disk_inode.set_mode(vfs_inode.mode() as u32);
        disk_inode.set_uid(vfs_inode.uid());
        disk_inode.set_gid(vfs_inode.gid());
        disk_inode.set_nlink(vfs_inode.nlink());
        disk_inode.set_atime(atime_sec);
        disk_inode.set_ctime(ctime_sec);
        disk_inode.set_mtime(mtime_sec);
        disk_inode.set_data_block_num(vfs_inode.data().data_blk_num());

        Ok(disk_inode)
    }

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

    pub(crate) fn set_mode(mut self, mode: u32) -> Self {
        self.mode = mode;

        self
    }

    pub(crate) fn set_uid(mut self, uid: u32) -> Self {
        self.uid = uid;

        self
    }

    pub(crate) fn set_gid(mut self, gid: u32) -> Self {
        self.gid = gid;

        self
    }

    pub(crate) fn set_atime(mut self, tv_sec: i64) -> Self {
        self.i_atime = tv_sec;

        self
    }

    pub(crate) fn set_mtime(mut self, tv_sec: i64) -> Self {
        self.i_mtime = tv_sec;

        self
    }

    pub(crate) fn set_ctime(mut self, tv_sec: i64) -> Self {
        self.i_ctime = tv_sec;

        self
    }

    pub(crate) fn set_nlink(mut self, nlink: u32) -> Self {
        self.nlink = nlink;

        self
    }

    pub(crate) fn set_data_block_num(mut self, data_block_num: u64) -> Self {
        self.data_blk_num = data_block_num;

        self
    }

    pub(crate) fn set_file_size(mut self, file_size: u64) -> Self {
        self.file_size = file_size;

        self
    }

    pub(crate) fn set_nblocks(mut self, nblocks: u64) -> Self {
        self.nblocks = nblocks;

        self
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

impl DerefMut for InodeStore {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inodes
    }
}

// SAFETY: EzfsInode is FromBytes, so array of them is too
unsafe impl FromBytes for InodeStore {}
unsafe impl AsBytes for InodeStore {}
