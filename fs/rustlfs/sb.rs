use crate::defs::{EZFS_BLOCK_SIZE, EZFS_MAX_DATA_BLKS, EZFS_MAX_INODES};
use core::mem::size_of;
use kernel::sync::Mutex;

// TODO: assert size is equal to 4096 bytes
#[repr(C)]
pub(crate) struct EzfsSuperblockDisk {
    magic: u64,
    disk_blocks: u64,
    free_inodes: [u8; EZFS_MAX_INODES],
    free_data_blocks: [u8; EZFS_MAX_DATA_BLKS],
    zero_data_blocks: [u8; EZFS_MAX_DATA_BLKS],
    padding:
        [u8; EZFS_BLOCK_SIZE - EZFS_MAX_INODES - EZFS_MAX_DATA_BLKS * 2 - size_of::<u64>() * 2],
}

// TODO: pin data because of mutexes
// in-memory representation of sb
pub(crate) struct EzfsSuperblock {
    magic: u64,
    disk_blocks: u64,
    free_inodes: Mutex<[u8; EZFS_MAX_INODES]>,
    free_data_blocks: Mutex<[u8; EZFS_MAX_DATA_BLKS]>,
    zero_data_blocks: Mutex<[u8; EZFS_MAX_DATA_BLKS]>,
}
