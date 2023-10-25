// SPDX-License-Identifier: GPL-2.0

//! Definitions of tarfs structures.

use kernel::types::LE;

/// Flags used in [`Inode::flags`].
pub mod inode_flags {
    /// Indicates that the inode is opaque.
    ///
    /// When set, inode will have the "trusted.overlay.opaque" set to "y" at runtime.
    pub const OPAQUE: u8 = 0x1;
}

kernel::derive_readable_from_bytes! {
    /// An inode in the tarfs inode table.
    #[repr(C)]
    pub struct Inode {
        /// The mode of the inode.
        ///
        /// The bottom 9 bits are the rwx bits for owner, group, all.
        ///
        /// The bits in the [`S_IFMT`] mask represent the file mode.
        pub mode: LE<u16>,

        /// Tarfs flags for the inode.
        ///
        /// Values are drawn from the [`inode_flags`] module.
        pub flags: u8,

        /// The bottom 4 bits represent the top 4 bits of mtime.
        pub hmtime: u8,

        /// The owner of the inode.
        pub owner: LE<u32>,

        /// The group of the inode.
        pub group: LE<u32>,

        /// The bottom 32 bits of mtime.
        pub lmtime: LE<u32>,

        /// Size of the contents of the inode.
        pub size: LE<u64>,

        /// Either the offset to the data, or the major and minor numbers of a device.
        ///
        /// For the latter, the 32 LSB are the minor, and the 32 MSB are the major numbers.
        pub offset: LE<u64>,
    }

    /// An entry in a tarfs directory entry table.
    #[repr(C)]
    pub struct DirEntry {
        /// The inode number this entry refers to.
        pub ino: LE<u64>,

        /// The offset to the name of the entry.
        pub name_offset: LE<u64>,

        /// The length of the name of the entry.
        pub name_len: LE<u64>,

        /// The type of entry.
        pub etype: u8,

        /// Unused padding.
        pub _padding: [u8; 7],
    }

    /// The super-block of a tarfs instance.
    #[repr(C)]
    pub struct Header {
        /// The offset to the beginning of the inode-table.
        pub inode_table_offset: LE<u64>,

        /// The number of inodes in the file system.
        pub inode_count: LE<u64>,
    }
}
