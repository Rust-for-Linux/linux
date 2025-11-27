use crate::defs::{EZFS_FILENAME_BUF_SIZE, EZFS_MAX_CHILDREN};
use core::ops::Deref;
use kernel::transmute::FromBytes;

#[repr(C)]
pub(crate) struct EzfsDirEntry {
    inode_no: u64,
    active: u8,
    filename: [u8; EZFS_FILENAME_BUF_SIZE],
}

impl EzfsDirEntry {
    pub(crate) fn inode_no(&self) -> u64 {
        self.inode_no
    }

    pub(crate) fn is_active(&self) -> bool {
        self.active != 0
    }

    pub(crate) fn filename(&self) -> &[u8] {
        let len = self
            .filename
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(self.filename.len());

        &self.filename[..len]
    }
}

#[repr(C)]
pub(crate) struct DirEntryStore {
    dir_entries: [EzfsDirEntry; EZFS_MAX_CHILDREN],
}

impl Deref for DirEntryStore {
    type Target = [EzfsDirEntry];

    fn deref(&self) -> &Self::Target {
        &self.dir_entries
    }
}

// TODO: Add Safety
unsafe impl FromBytes for DirEntryStore {}
