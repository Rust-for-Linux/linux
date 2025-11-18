use crate::dir::EzfsDirEntry;
use crate::inode::EzfsInode;
use core::mem::size_of;

pub(crate) const EZFS_FILENAME_BUF_SIZE: usize = 128 - 8 - 1;
pub(crate) const EZFS_FILENAME_LENGTH: usize = EZFS_FILENAME_BUF_SIZE - 1;

pub(crate) const EZFS_MAGIC_NUMBER: usize = 0x00004118;
pub(crate) const EZFS_BLOCK_SIZE: usize = 4096;
pub(crate) const EZFS_ROOT_INODE_NUMBER: usize = 1;
pub(crate) const EZFS_SUPERBLOCK_DATABLOCK_NUMBER: usize = 0;
pub(crate) const EZFS_INODE_STORE_DATABLOCK_NUMBER: usize = 1;
pub(crate) const EZFS_ROOT_DATABLOCK_NUMBER: usize = 2;

pub(crate) const EZFS_MAX_INODES: usize = EZFS_BLOCK_SIZE / size_of::<EzfsInode>();
pub(crate) const EZFS_MAX_DATA_BLKS: usize = EZFS_MAX_INODES * 256;
pub(crate) const EZFS_MAX_CHILDREN: usize = EZFS_BLOCK_SIZE / size_of::<EzfsDirEntry>();
