use crate::defs::{EZFS_BLOCK_SIZE, EZFS_MAX_DATA_BLKS, EZFS_MAX_INODES};
use core::mem::size_of;
use kernel::sync::Mutex;
use kernel::transmute::FromBytes;

pub(crate) struct EzfsSuperblockDiskRaw {
    magic: u64,
    disk_blocks: u64,
    free_inodes: [u32; (EZFS_MAX_INODES / 32) + 1],
    free_data_blocks: [u32; (EZFS_MAX_DATA_BLKS / 32) + 1],
    zero_data_blocks: [u8; (EZFS_MAX_DATA_BLKS / 32) + 1],
}

// TODO: assert size is equal to 4096 bytes
#[repr(C)]
pub(crate) struct EzfsSuperblockDisk {
    data: EzfsSuperblockDiskRaw,
    _padding: [u8; EZFS_BLOCK_SIZE - size_of::<EzfsSuperblockDiskRaw>()],
}

impl EzfsSuperblockDisk {
    pub fn magic(&self) -> u64 {
        self.data.magic
    }
}

// SAFETY: EzfsSuperblockDisk contains only primitive integer types (u32, u64, u8)
// which accept any bit pattern. The struct is #[repr(C)] for consistent layout.
unsafe impl FromBytes for EzfsSuperblockDisk {}

// TODO: pin data because of mutexes
// in-memory representation of sb
pub(crate) struct EzfsSuperblock {
    magic: u64,
    disk_blocks: u64,
    free_inodes: Mutex<[u32; (EZFS_MAX_INODES / 32) + 1]>,
    free_data_blocks: Mutex<[u32; (EZFS_MAX_DATA_BLKS / 32) + 1]>,
    zero_data_blocks: Mutex<[u8; (EZFS_MAX_DATA_BLKS / 32) + 1]>,
}
