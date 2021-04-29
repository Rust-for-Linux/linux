// SPDX-License-Identifier: GPL-2.0

use crate::bindings;
use crate::c_types;
use crate::error::{Error, KernelResult};

pub struct Registration {
    this_module: &'static crate::ThisModule,
    spi_driver: bindings::spi_driver,
    prout
}

impl Registration {
    pub fn new(this_module: &'static crate::ThisModule,
               spi_driver: bindings::spi_driver
               ) -> Self {
        Registration {
            this_module,
            spi_driver
        }
    }

    pub fn register(&mut self) -> KernelResult {
        let res = unsafe { bindings::__spi_register_driver(self.this_module.0, &mut self.spi_driver as
                                                            *mut bindings::spi_driver) };

        if res != 0 {
            return Err(Error::from_kernel_errno(res));
        }

        Ok(())
    }
}
