use crate::defs::{EZFS_FILENAME_BUF_SIZE, EZFS_MAX_CHILDREN};
use core::ops::{Deref, DerefMut};
use kernel::{
    prelude::*,
    transmute::{AsBytes, FromBytes},
};

#[repr(C)]
#[derive(Clone, Copy)]
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

    pub(crate) fn set_inode_no(&mut self, ino: u64) -> &mut Self {
        self.inode_no = ino;

        self
    }

    pub(crate) fn set_active(&mut self) -> &mut Self {
        self.active = 1;

        self
    }

    pub(crate) fn set_filename(&mut self, filename: &[u8]) -> Result<&mut Self> {
        if filename.len() > self.filename.len() {
            return Err(ENAMETOOLONG);
        }

        self.filename[..filename.len()].copy_from_slice(filename);
        self.filename[filename.len()..].fill(0);

        Ok(self)
    }

    pub(crate) fn zero(&mut self) -> &mut Self {
        let len = self.filename.len();

        self.inode_no = 0;
        self.active = 0;
        self.filename[..len].fill(0);

        self
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

impl DerefMut for DirEntryStore {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.dir_entries
    }
}

// TODO: Add Safety
unsafe impl FromBytes for DirEntryStore {}
unsafe impl AsBytes for DirEntryStore {}
