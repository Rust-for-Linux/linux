use crate::defs::{EZFS_BLOCK_SIZE, EZFS_MAX_DATA_BLKS, EZFS_MAX_INODES};
use crate::inode::InodeStore;
use crate::RustEzFs;
use core::mem::size_of;
use core::ops::{Deref, DerefMut};
use kernel::fs::FileSystem;
use kernel::new_mutex;
use kernel::prelude::*;
use kernel::sync::{
    lock::{mutex::MutexBackend, Guard},
    Mutex,
};
use kernel::transmute::FromBytes;
use kernel::{block, inode};

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
    pub(crate) fn magic(&self) -> u64 {
        self.data.magic
    }
}

// SAFETY: EzfsSuperblockDisk contains only primitive integer types (u32, u64, u8)
// which accept any bit pattern. The struct is #[repr(C)] for consistent layout.
unsafe impl FromBytes for EzfsSuperblockDisk {}

#[repr(transparent)]
pub(crate) struct Bitmap<const N: usize> {
    inner: [u32; N],
}

impl<const N: usize> Bitmap<N> {
    #[inline]
    pub(crate) fn is_set(&self, block_num: u64) -> bool {
        let idx: usize = (block_num / 32) as usize;
        let mask = 1 << (block_num % 32);
        (self.inner[idx] & mask) != 0
    }

    #[inline]
    pub(crate) fn set_bit(&mut self, block_num: u64) -> () {
        let idx: usize = (block_num / 32) as usize;
        let mask = 1 << (block_num % 32);
        self.inner[idx] |= mask
    }

    #[inline]
    pub(crate) fn clear_bit(&mut self, block_num: u64) -> () {
        let idx: usize = (block_num / 32) as usize;
        let mask = 1 << (block_num % 32);
        self.inner[idx] &= !mask
    }

    const fn new(inner: [u32; N]) -> Self {
        Self { inner }
    }
}

impl<const N: usize> Deref for Bitmap<N> {
    type Target = [u32; N];

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<const N: usize> DerefMut for Bitmap<N> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

#[pin_data]
pub(crate) struct EzfsSuperblock {
    pub(crate) version: u64,
    pub(crate) magic: u64,
    pub(crate) disk_blocks: u64,
    #[pin]
    pub(crate) free_inodes: Mutex<[u32; (EZFS_MAX_INODES / 32) + 1]>,
    #[pin]
    pub(crate) free_data_blocks: Mutex<Bitmap<{ (EZFS_MAX_DATA_BLKS / 32) + 1 }>>,
    #[pin]
    pub(crate) zero_data_blocks: Mutex<[u8; (EZFS_MAX_DATA_BLKS / 32) + 1]>,
    #[pin]
    sb_lock: Mutex<()>,
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
            free_data_blocks <- new_mutex!(Bitmap::new(disk_sb.data.free_data_blocks)),
            zero_data_blocks <- new_mutex!(disk_sb.data.zero_data_blocks),
            sb_lock <- new_mutex!(()),
            mapper,
        })
    }

    pub(crate) fn magic(&self) -> u64 {
        self.magic
    }

    pub(crate) fn lock(&self) -> Guard<'_, (), MutexBackend> {
        self.sb_lock.lock()
    }
}
