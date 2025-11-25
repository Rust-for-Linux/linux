use crate::defs::{EZFS_BLOCK_SIZE, EZFS_MAX_DATA_BLKS, EZFS_MAX_INODES};
use crate::inode::InodeStore;
use crate::RustEzFs;
use core::mem::size_of;
use kernel::fs::FileSystem;
use kernel::inode;
use kernel::new_mutex;
use kernel::prelude::*;
use kernel::sync::Mutex;
use kernel::transmute::FromBytes;

#[repr(C)]
pub(crate) struct EzfsSuperblockDiskRaw {
    version: u64,
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

#[pin_data]
pub(crate) struct EzfsSuperblock {
    pub(crate) version: u64,
    pub(crate) magic: u64,
    pub(crate) disk_blocks: u64,
    #[pin]
    pub(crate) free_inodes: Mutex<[u32; (EZFS_MAX_INODES / 32) + 1]>,
    #[pin]
    pub(crate) free_data_blocks: Mutex<[u32; (EZFS_MAX_DATA_BLKS / 32) + 1]>,
    #[pin]
    pub(crate) zero_data_blocks: Mutex<[u8; (EZFS_MAX_DATA_BLKS / 32) + 1]>,
    pub(crate) mapper: inode::Mapper<RustEzFs>,
}

impl EzfsSuperblock {
    pub(crate) fn new(
        disk_sb: EzfsSuperblockDisk,
        mapper: inode::Mapper<RustEzFs>,
    ) -> impl PinInit<Self> {
        pin_init!(Self {
            version: disk_sb.data.version,
            magic: disk_sb.data.magic,
            disk_blocks: disk_sb.data.disk_blocks,
            free_inodes <- new_mutex!(disk_sb.data.free_inodes),
            free_data_blocks <- new_mutex!(disk_sb.data.free_data_blocks),
            zero_data_blocks <- new_mutex!(disk_sb.data.zero_data_blocks),
            mapper,
        })
    }

    pub(crate) fn magic(&self) -> u64 {
        self.magic
    }
}
