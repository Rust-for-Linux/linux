// SPDX-License-Identifier: GPL-2.0

#![no_std]
#![feature(global_asm)]

use kernel::prelude::*;
use kernel::sync::Mutex;

module! {
    type: RustExample,
    name: b"rust_example",
    author: b"Rust for Linux Contributors",
    description: b"An example kernel module written in Rust",
    license: b"GPL v2",
    params: {
        my_bool: bool {
            default: true,
            permissions: 0,
            description: b"Example of bool",
        },
        my_i32: i32 {
            default: 42,
            permissions: 0o644,
            description: b"Example of i32",
        },
    },
}

struct RustExample {
    message: String,
}

impl KernelModule for RustExample {
    fn init() -> KernelResult<Self> {
        println!("Rust Example (init)");
        println!("Am I built-in? {}", !cfg!(MODULE));
        println!("Parameters:");
        println!("  my_bool:  {}", my_bool.read());
        println!("  my_i32:   {}", my_i32.read());

        let mutex = Mutex::new(1, "mutex");
        println!("mutex: {:?}", mutex);
        unsafe {
            let guard = mutex.lock();
            println!("data: {}", guard);
        }

        Ok(RustExample {
            message: "on the heap!".to_owned(),
        })
    }
}

impl Drop for RustExample {
    fn drop(&mut self) {
        println!("My message is {}", self.message);
        println!("Rust Example (exit)");
    }
}
