use crate::{error::{from_result, Result}, fs::file::File, fs::FileSystem, folio::Folio, folio::PageCache, types::Locked};
use core::marker::PhantomData;
use crate::prelude::EIO;
use macros::vtable;

/// Operations implemented by address space
#[vtable]
pub trait Operations {
    /// File system that these operations are compatible with.
    type FileSystem: FileSystem + ?Sized;

    fn read_folio(
        file: Option<&File<Self::FileSystem>>,
        // folio: Locked<&Folio<PageCache<Self::FileSystem>>>,
        folio: &Folio<PageCache<Self::FileSystem>>,
    ) -> Result;
}

/// Represents address space operations.
pub struct Ops<T: FileSystem + ?Sized>(
    pub(crate) *const bindings::address_space_operations,
    pub(crate) PhantomData<T>,
);


impl <T: FileSystem + ?Sized> Ops<T> {

    pub const fn new<U: Operations<FileSystem = T> + ?Sized>() -> Self {
        struct Table<T: Operations + ?Sized>(PhantomData<T>);
        impl<T: Operations + ?Sized> Table<T> {

            const TABLE: bindings::address_space_operations = bindings::address_space_operations {
                read_folio: Some(Self::read_folio_callback),
                writepages: None,
                dirty_folio: None,
                readahead: None,
                write_begin: None,
                write_end: None,
                bmap: None,
                invalidate_folio: None,
                release_folio: None,
                free_folio: None,
                direct_IO: None,
                migrate_folio: None,
                launder_folio: None,
                is_partially_uptodate: None,
                is_dirty_writeback: None,
                error_remove_folio: None,
                swap_activate: None,
                swap_deactivate: None,
                swap_rw: None,
            };

            extern "C" fn read_folio_callback(
                file_ptr: *mut bindings::file,
                folio_ptr: *mut bindings::folio,
            ) -> i32 {
                from_result(|| {
                    Err(EIO)
                })
            }
        }
        Self(&Table::<U>::TABLE, PhantomData)
    }
}
