// SPDX-License-Identifier: GPL-2.0

//! Rust I2C client sample.

use kernel::{i2c, of, prelude::*};

kernel::module_i2c_driver! {
    type: Driver,
    name: "rust_i2c_client",
    license: "GPL",
}

kernel::module_i2c_id_table!(I2C_MOD_TABLE, I2C_CLIENT_I2C_ID_TABLE);
kernel::define_i2c_id_table! {I2C_CLIENT_I2C_ID_TABLE, (), [
    (i2c::DeviceId(b"rust-sample-i2c"), None),
]}

kernel::module_of_id_table!(OF_MOD_TABLE, I2C_CLIENT_OF_ID_TABLE);
kernel::define_of_id_table! {I2C_CLIENT_OF_ID_TABLE, (), [
    (of::DeviceId::Compatible(b"rust,rust-sample-i2c"), None),
]}

struct Driver;
impl i2c::Driver for Driver {
    kernel::driver_i2c_id_table!(I2C_CLIENT_I2C_ID_TABLE);
    kernel::driver_of_id_table!(I2C_CLIENT_OF_ID_TABLE);

    fn probe(_client: &mut i2c::Client) -> Result {
        Ok(())
    }
}
