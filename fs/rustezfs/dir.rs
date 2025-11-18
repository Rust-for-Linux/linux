use crate::defs::EZFS_FILENAME_BUF_SIZE;

#[repr(C)]
pub(crate) struct EzfsDirEntry {
    inode_no: u64,
    active: u8,
    filename: [char; EZFS_FILENAME_BUF_SIZE],
}
