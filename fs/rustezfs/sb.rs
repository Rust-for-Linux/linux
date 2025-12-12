use crate::defs::{
    EZFS_BLOCK_SIZE, EZFS_MAX_DATA_BLKS, EZFS_MAX_INODES, EZFS_ROOT_DATABLOCK_NUMBER,
    EZFS_ROOT_INODE_NUMBER,
};
use crate::RustEzFs;
use core::mem::size_of;
use core::ops::{Deref, DerefMut};
use kernel::folio::Folio;
use kernel::inode;
use kernel::new_mutex;
use kernel::prelude::*;
use kernel::sb::SuperBlock;
use kernel::sync::Mutex;
use kernel::transmute::{AsBytes, FromBytes};
use kernel::types::{ARef, Lockable};

#[repr(C)]
pub(crate) struct EzfsSuperblockDiskRaw {
    version: u64,
    magic: u64,
    disk_blocks: u64,
    free_inodes: [u32; (EZFS_MAX_INODES / 32) + 1],
    free_data_blocks: [u32; (EZFS_MAX_DATA_BLKS / 32) + 1],
    zero_data_blocks: [u32; (EZFS_MAX_DATA_BLKS / 32) + 1],
}

// TODO: assert size is equal to 4096 bytes
#[repr(C)]
pub(crate) struct EzfsSuperblockDisk {
    data: EzfsSuperblockDiskRaw,
    _padding: [u8; EZFS_BLOCK_SIZE - size_of::<EzfsSuperblockDiskRaw>()],
}

impl Default for EzfsSuperblockDiskRaw {
    fn default() -> Self {
        Self {
            version: 0,
            magic: 0,
            disk_blocks: 0,
            free_inodes: [0; (EZFS_MAX_INODES / 32) + 1],
            free_data_blocks: [0; (EZFS_MAX_DATA_BLKS / 32) + 1],
            zero_data_blocks: [0; (EZFS_MAX_DATA_BLKS / 32) + 1],
        }
    }
}

impl Default for EzfsSuperblockDisk {
    fn default() -> Self {
        Self {
            data: EzfsSuperblockDiskRaw::default(),
            _padding: [0; EZFS_BLOCK_SIZE - size_of::<EzfsSuperblockDiskRaw>()],
        }
    }
}

impl EzfsSuperblockDisk {
    pub(crate) fn magic(&self) -> u64 {
        self.data.magic
    }
}

// SAFETY: EzfsSuperblockDisk contains only primitive integer types (u32, u64, u8)
// which accept any bit pattern. The struct is #[repr(C)] for consistent layout.
unsafe impl FromBytes for EzfsSuperblockDisk {}
unsafe impl AsBytes for EzfsSuperblockDisk {}

#[repr(transparent)]
pub(crate) struct Bitmap<const N: usize> {
    inner: [u32; N],
}

impl<const N: usize> Bitmap<N> {
    #[inline]
    pub(crate) fn is_set(&self, block_num: u64) -> bool {
        let idx = (block_num / 32) as usize;
        if idx >= N {
            return false;
        }

        let mask = 1 << (block_num % 32);
        (self.inner[idx] & mask) != 0
    }

    #[inline]
    pub(crate) fn set_bit(&mut self, block_num: u64) -> Result {
        let idx = (block_num / 32) as usize;
        let mask = 1 << (block_num % 32);
        let val = self.inner.get_mut(idx).ok_or(EINVAL)?;
        *val |= mask;

        Ok(())
    }

    #[inline]
    pub(crate) fn clear_bit(&mut self, block_num: u64) -> Result {
        let idx: usize = (block_num / 32) as usize;
        let mask = 1 << (block_num % 32);
        let val = self.inner.get_mut(idx).ok_or(EINVAL)?;
        *val &= !mask;

        Ok(())
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
    pub(crate) data: Mutex<EzfsSuperblockData>,
    pub(crate) mapper: inode::Mapper<RustEzFs>,
}

pub(crate) struct EzfsSuperblockData {
    pub(crate) free_inodes: Bitmap<{ (EZFS_MAX_INODES / 32) + 1 }>,
    pub(crate) free_data_blocks: Bitmap<{ (EZFS_MAX_DATA_BLKS / 32) + 1 }>,
    pub(crate) zero_data_blocks: Bitmap<{ (EZFS_MAX_DATA_BLKS / 32) + 1 }>,
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
            data <- new_mutex!(EzfsSuperblockData {
                free_inodes: Bitmap::new(disk_sb.data.free_inodes),
                free_data_blocks: Bitmap::new(disk_sb.data.free_data_blocks),
                zero_data_blocks: Bitmap::new(disk_sb.data.zero_data_blocks),
            }),
            mapper,
        })
    }

    pub(crate) fn to_disk(&self) -> EzfsSuperblockDisk {
        let mut disk_sb = EzfsSuperblockDisk::default();

        disk_sb.data.version = self.version;
        disk_sb.data.magic = self.magic;
        disk_sb.data.disk_blocks = self.disk_blocks;

        let sb_data = self.data.lock();
        disk_sb.data.free_inodes = *sb_data.free_inodes;
        disk_sb.data.free_data_blocks = *sb_data.free_data_blocks;
        disk_sb.data.zero_data_blocks = *sb_data.zero_data_blocks;

        disk_sb
    }
}

pub(crate) struct Transaction<'a> {
    sb: &'a SuperBlock<RustEzFs>,
    allocated_inodes: KVec<u64>,
    allocated_blocks: KVec<u64>,
    committed: bool,
}

impl<'a> Transaction<'a> {
    pub(crate) fn new(sb: &'a SuperBlock<RustEzFs>) -> Self {
        Self {
            sb,
            allocated_inodes: Vec::new(),
            allocated_blocks: Vec::new(),
            committed: false,
        }
    }

    pub(crate) fn allocate_inode(&mut self) -> Result<usize> {
        let ezfs_sb = self.sb.data();
        let mut sb_data = ezfs_sb.data.lock();

        for idx in 0..EZFS_MAX_INODES {
            if !sb_data.free_inodes.is_set(idx as u64) {
                sb_data.free_inodes.set_bit(idx as u64)?;
                self.allocated_inodes.push(idx as u64, GFP_KERNEL)?;
                return Ok(idx + EZFS_ROOT_INODE_NUMBER);
            }
        }

        Err(ENOSPC)
    }

    pub(crate) fn allocate_data_block(&mut self, max_blocks: u64) -> Result<u64> {
        let ezfs_sb = self.sb.data();

        let mut sb_data = ezfs_sb.data.lock();

        for idx in 0..max_blocks {
            if !sb_data.free_data_blocks.is_set(idx) {
                sb_data.free_data_blocks.set_bit(idx)?;
                self.allocated_blocks.push(idx, GFP_KERNEL)?;
                return Ok(idx + EZFS_ROOT_DATABLOCK_NUMBER as u64);
            }
        }

        Err(ENOSPC)
    }

    pub(crate) fn zeroed_data_block(&mut self, max_blocks: u64) -> Result<u64> {
        let ezfs_sb = self.sb.data();
        let data_block_num = self.allocate_data_block(max_blocks)?;

        let offset = data_block_num
            .checked_mul(EZFS_BLOCK_SIZE as u64)
            .ok_or(EIO)?;

        let folio: ARef<Folio<kernel::folio::PageCache<RustEzFs>>> =
            ezfs_sb.mapper.read_mapping_folio(offset.try_into()?)?;

        let folio_start = 0;
        let locked_folio = folio.lock();
        let mut guard = locked_folio.map(folio_start)?;
        guard.fill(0);

        Ok(data_block_num)
    }

    pub(crate) fn commit(mut self) {
        self.committed = true;
    }
}

impl<'a> Drop for Transaction<'a> {
    fn drop(&mut self) {
        if !self.committed {
            pr_info!(
                "Rolling back transaction: {} inodes, {} blocks\n",
                self.allocated_inodes.len(),
                self.allocated_blocks.len()
            );

            let ezfs_sb = self.sb.data();
            let mut sb_data = ezfs_sb.data.lock();

            // SAFETY: Index is guaranteed valid since it was successfully allocated
            for &idx in &self.allocated_blocks {
                let _ = sb_data.free_data_blocks.clear_bit(idx);
                let _ = sb_data.zero_data_blocks.clear_bit(idx);
            }

            for &idx in &self.allocated_inodes {
                let _ = sb_data.free_inodes.clear_bit(idx);
            }
        }
    }
}
