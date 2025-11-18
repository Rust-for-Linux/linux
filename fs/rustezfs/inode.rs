use kernel::uapi::{gid_t, mode_t, uid_t};

#[repr(C)]
pub(crate) struct EzfsInode {
    mode: mode_t,
    uid: uid_t,
    gid: gid_t,
    i_attime: i64, /* access time */
    i_mtime: i64,  /* modified time */
    i_ctime: i64,  /* change time */
    nlink: u32,
    data_blk_num: u64,
    file_size: u64,
    nblocks: u64,
}
