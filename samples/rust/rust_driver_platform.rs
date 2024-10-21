// SPDX-License-Identifier: GPL-2.0

//! Rust Platform driver sample.

use kernel::{c_str, of, platform, prelude::*};

struct SampleDriver {
    pdev: platform::Device,
}

struct Info(u32);

kernel::of_device_table!(
    OF_TABLE,
    MODULE_OF_TABLE,
    <SampleDriver as platform::Driver>::IdInfo,
    [(
        of::DeviceId::new(c_str!("redhat,rust-sample-platform-driver")),
        Info(42)
    )]
);

impl platform::Driver for SampleDriver {
    type IdInfo = Info;
    const ID_TABLE: platform::IdTable<Self::IdInfo> = &OF_TABLE;

    fn probe(pdev: &mut platform::Device, info: Option<&Self::IdInfo>) -> Result<Pin<KBox<Self>>> {
        dev_dbg!(pdev.as_ref(), "Probe Rust Platform driver sample.\n");

        match (Self::of_match_device(pdev), info) {
            (Some(id), Some(info)) => {
                dev_info!(
                    pdev.as_ref(),
                    "Probed by OF compatible match: '{}' with info: '{}'.\n",
                    id.compatible(),
                    info.0
                );
            }
            _ => {
                dev_info!(pdev.as_ref(), "Probed by name.\n");
            }
        };

        let drvdata = KBox::new(Self { pdev: pdev.clone() }, GFP_KERNEL)?;

        Ok(drvdata.into())
    }
}

impl Drop for SampleDriver {
    fn drop(&mut self) {
        dev_dbg!(self.pdev.as_ref(), "Remove Rust Platform driver sample.\n");
    }
}

kernel::module_platform_driver! {
    type: SampleDriver,
    name: "rust_driver_platform",
    author: "Danilo Krummrich",
    description: "Rust Platform driver",
    license: "GPL v2",
}
