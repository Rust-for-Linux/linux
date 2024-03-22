// SPDX-License-Identifier: GPL-2.0

//! SoC Regulator Driver Interface
//!
//! C header: [`include/linux/regulator/driver.h`](srctree/include/linux/regulator/driver.h)
//!
//! # Examples
//!
//! ```ignore
//! use kernel::regulator::driver::{Config, Desc, Driver, Registration, Type};
//! # use kernel::{c_str, device::Device, module_platform_driver, of, platform, prelude::*};
//! #
//! # module_platform_driver! {
//! #     type: MyRegulatorDriver,
//! #     name: "my_regulator_driver",
//! #     license: "GPL",
//! # }
//! #
//! # kernel::module_of_id_table!(OF_MOD_TABLE, MY_REG_DRIVER_OF_ID_TABLE);
//! # kernel::define_of_id_table! {MY_REG_DRIVER_OF_ID_TABLE, (), [
//! #   (of::DeviceId::Compatible(b"my-regulator-driver"), None),
//! # ]}
//!
//! static DESC: Desc =
//!     Desc::new::<MyRegulatorDriver>(c_str!("my-regulator-driver"), Type::Voltage)
//!         .with_owner(&THIS_MODULE);
//!
//! struct MyRegulatorDriver;
//!
//! impl platform::Driver for MyRegulatorDriver {
//! #   kernel::driver_of_id_table!(MY_REG_DRIVER_OF_ID_TABLE);
//! #
//!     fn probe(pdev: &mut platform::Device, _id_info: Option<&Self::IdInfo>) -> Result {
//!         let reg = Registration::register(pdev, &DESC, Config::<Self::Data>::new(pdev))?;
//!         Ok(())
//!     }
//! }
//!
//! #[vtable]
//! impl Driver for MyRegulatorDriver {
//!     // Implement supported `Driver`'s operations here.
//! }
//! ```

use crate::{
    device::RawDevice,
    error::{code::*, from_err_ptr, from_result, Error, Result},
    macros::vtable,
    regulator::Mode,
    str::CStr,
    sync::Arc,
    types::ForeignOwnable,
    ThisModule,
};
#[cfg(CONFIG_REGMAP)]
use crate::{error::to_result, regmap::Regmap};
use core::{
    marker::PhantomData,
    mem::{ManuallyDrop, MaybeUninit},
};

#[cfg(not(CONFIG_REGMAP))]
struct Regmap;

#[cfg(not(CONFIG_REGMAP))]
impl Regmap {
    fn as_ptr(&self) -> *mut bindings::regmap {
        core::ptr::null_mut()
    }
}

/// [`Regulator`]'s status
///
/// Corresponds to the kernel's [`enum regulator_status`].
///
/// [`enum regulator_status`]: srctree/include/linux/regulator/driver.h
#[derive(Eq, PartialEq)]
pub enum Status {
    /// Regulator is off
    Off,
    /// Regulator is on
    On,
    /// Regulator is in an error state
    Error,
    /// Regulator is on and in Fast mode
    Fast,
    /// Regulator is on and in Normal mode
    Normal,
    /// Regulator is on and in Idle mode
    Idle,
    /// Regulator is on and in Standby mode
    Standby,
    /// Regulator is enabled but not regulating
    Bypass,
    /// Regulator is any other status
    Undefined,
}

impl TryFrom<core::ffi::c_uint> for Status {
    type Error = Error;

    fn try_from(status: core::ffi::c_uint) -> Result<Self> {
        match status {
            bindings::regulator_status_REGULATOR_STATUS_OFF => Ok(Self::Off),
            bindings::regulator_status_REGULATOR_STATUS_ON => Ok(Self::On),
            bindings::regulator_status_REGULATOR_STATUS_ERROR => Ok(Self::Error),
            bindings::regulator_status_REGULATOR_STATUS_FAST => Ok(Self::Fast),
            bindings::regulator_status_REGULATOR_STATUS_NORMAL => Ok(Self::Normal),
            bindings::regulator_status_REGULATOR_STATUS_IDLE => Ok(Self::Idle),
            bindings::regulator_status_REGULATOR_STATUS_STANDBY => Ok(Self::Standby),
            bindings::regulator_status_REGULATOR_STATUS_BYPASS => Ok(Self::Bypass),
            bindings::regulator_status_REGULATOR_STATUS_UNDEFINED => Ok(Self::Undefined),
            _ => Err(EINVAL),
        }
    }
}

impl From<Mode> for Status {
    fn from(mode: Mode) -> Self {
        // SAFETY: `regulator_mode_to_status` is a pure function that is only doing integer
        // to integer conversion, hence this function call is safe.
        let status = unsafe { bindings::regulator_mode_to_status(mode as _) };

        if status < 0 {
            Self::Undefined
        } else {
            Self::try_from(status as core::ffi::c_uint).unwrap_or(Self::Undefined)
        }
    }
}

/// Regulator's operations
#[vtable]
pub trait Driver {
    /// User data that will be accessible to all operations
    type Data: ForeignOwnable + Send + Sync = ();

    /// Return one of the supported voltages, in microvolt; zero if the selector indicates a
    /// voltage that is unusable by the system; or negative errno. Selectors range from zero to one
    /// less than the number of voltages supported by the system.
    fn list_voltage(_rdev: &Regulator, _selector: u32) -> Result<i32> {
        Err(ENOTSUPP)
    }

    /// Set the voltage for the regulator within the range specified. The driver should select the
    /// voltage closest to `min_uv`.
    fn set_voltage(_rdev: &Regulator, _min_uv: i32, _max_uv: i32) -> Result<i32> {
        Err(ENOTSUPP)
    }

    /// Set the voltage for the regulator using the specified selector.
    fn set_voltage_sel(_rdev: &Regulator, _selector: u32) -> Result {
        Err(ENOTSUPP)
    }

    /// Convert a voltage into a selector.
    fn map_voltage(_rdev: &Regulator, _min_uv: i32, _max_uv: i32) -> Result<i32> {
        Err(ENOTSUPP)
    }

    /// Get the currently configured voltage for the regulator; Returns
    /// [`ENOTRECOVERABLE`] if the regulator can't be read at bootup and hasn't been
    /// set yet.
    fn get_voltage(_rdev: &Regulator) -> Result<i32> {
        Err(ENOTSUPP)
    }

    /// Get the currently configured voltage selector for the regulator; Returns
    /// [`ENOTRECOVERABLE`] if the regulator can't be read at bootup and hasn't been
    /// set yet.
    fn get_voltage_sel(_rdev: &Regulator) -> Result<i32> {
        Err(ENOTSUPP)
    }

    /// Configure a limit for a current-limited regulator.
    ///
    /// The driver should select the current closest to `max_ua`.
    fn set_current_limit(_rdev: &Regulator, _min_ua: i32, _max_ua: i32) -> Result {
        Err(ENOTSUPP)
    }

    /// Get the configured limit for a current-limited regulator.
    fn get_current_limit(_rdev: &Regulator) -> Result<i32> {
        Err(ENOTSUPP)
    }

    /// Enable or disable the active discharge of the regulator.
    fn set_active_discharge(_rdev: &Regulator, _enable: bool) -> Result {
        Err(ENOTSUPP)
    }

    /// Configure the regulator as enabled.
    fn enable(_rdev: &Regulator) -> Result {
        Err(ENOTSUPP)
    }

    /// Configure the regulator as disabled.
    fn disable(_rdev: &Regulator) -> Result {
        Err(ENOTSUPP)
    }

    /// Returns enablement state of the regulator.
    fn is_enabled(_rdev: &Regulator) -> Result<bool> {
        Err(ENOTSUPP)
    }

    /// Set the configured operating [`Mode`] for the regulator.
    fn set_mode(_rdev: &Regulator, _mode: Mode) -> Result {
        Err(ENOTSUPP)
    }

    /// Get the configured operating [`Mode`] for the regulator
    fn get_mode(_rdev: &Regulator) -> Mode {
        Mode::Invalid
    }

    /// Report the regulator [`Status`].
    fn get_status(_rdev: &Regulator) -> Result<Status> {
        Err(ENOTSUPP)
    }

    /// Set the voltage for the regaultor when the system is suspended.
    fn set_suspend_voltage(_rdev: &Regulator, _uv: i32) -> Result {
        Err(ENOTSUPP)
    }

    /// Mark the regulator as enabled when the system is suspended.
    fn set_suspend_enable(_rdev: &Regulator) -> Result {
        Err(ENOTSUPP)
    }

    /// Mark the regulator as disabled when the system is suspended.
    fn set_suspend_disable(_rdev: &Regulator) -> Result {
        Err(ENOTSUPP)
    }

    /// Set the operating mode for the regulator when the system is suspended.
    fn set_suspend_mode(_rdev: &Regulator, _mode: Mode) -> Result {
        Err(ENOTSUPP)
    }
}

/// [`Regulator`]'s descriptor
///
/// # Invariants
///
/// `self.0` has always valid data.
pub struct Desc(bindings::regulator_desc);
impl Desc {
    /// Create a new [`Regulator`] descriptor
    pub const fn new<T: Driver>(name: &'static CStr, reg_type: Type) -> Self {
        // SAFETY: `desc` is a C structure holding data that has been initialized with 0s,
        // hence it is safe to use as-is.
        let mut desc = unsafe { MaybeUninit::<bindings::regulator_desc>::zeroed().assume_init() };
        desc.name = name.as_char_ptr();
        desc.type_ = match reg_type {
            Type::Voltage => bindings::regulator_type_REGULATOR_VOLTAGE,
            Type::Current => bindings::regulator_type_REGULATOR_CURRENT,
        };
        desc.ops = Adapter::<T>::build();
        Self(desc)
    }

    /// Setup the register address, mask, and {en,dis}able values
    pub const fn with_enable(mut self, reg: u32, mask: u32, en_val: u32, dis_val: u32) -> Self {
        self.0.enable_reg = reg;
        self.0.enable_mask = mask;
        self.0.enable_val = en_val;
        self.0.disable_val = dis_val;
        self
    }

    /// Setup the register address, mask, and {en,dis}able values. {En,Dis}able values are
    /// inverted, i.e. `dis_val` will be use to enable the regulator while `en_val` will be used
    /// to disable the regulator.
    pub const fn with_inverted_enable(
        mut self,
        reg: u32,
        mask: u32,
        en_val: u32,
        dis_val: u32,
    ) -> Self {
        self.0.enable_is_inverted = true;
        self.with_enable(reg, mask, en_val, dis_val)
    }

    /// Setup the active discharge regiter address, mask, on/off values.
    pub const fn with_active_discharge(mut self, reg: u32, mask: u32, on: u32, off: u32) -> Self {
        self.0.active_discharge_on = on;
        self.0.active_discharge_off = off;
        self.0.active_discharge_reg = reg;
        self.0.active_discharge_mask = mask;
        self
    }

    /// Setup the current selection register address, mask, and current table
    pub const fn with_csel(mut self, reg: u32, mask: u32, table: &'static [u32]) -> Self {
        self.0.csel_reg = reg;
        self.0.csel_mask = mask;
        self.0.curr_table = table.as_ptr();
        self
    }

    /// Voltages are a linear mapping
    pub const fn with_linear_mapping(
        mut self,
        reg: u32,
        mask: u32,
        min_uv: u32,
        uv_step: u32,
        n_voltages: u32,
        linear_min_sel: u32,
    ) -> Self {
        self.0.vsel_reg = reg;
        self.0.vsel_mask = mask;
        self.0.n_voltages = n_voltages;
        self.0.min_uV = min_uv;
        self.0.uV_step = uv_step;
        self.0.linear_min_sel = linear_min_sel;
        self
    }

    /// Set the regulator owner
    pub const fn with_owner(mut self, owner: &'static ThisModule) -> Self {
        self.0.owner = owner.as_ptr();
        self
    }
}
unsafe impl Sync for Desc {}

/// [`Regulator`]'s Config
///
/// # Examples
///
/// ```
/// use kernel::regulator::driver::Config;
/// # use kernel::regulator::driver::{Desc, Registration};
/// # use kernel::{device::Device, sync::Arc};
///
/// struct DriverData(u32);
///
/// # fn probe(dev: &Device, desc: &'static Desc) -> Result {
/// let config = Config::<Arc<DriverData>>::new(&dev, Arc::try_new(DriverData(128))?);
/// let reg = Registration::register(dev, desc, config)?;
/// #     Ok(())
/// # }
/// ```
///
/// # Invariants
///
/// `self.cfg` always hold valid data.
pub struct Config<T: ForeignOwnable + Send + Sync = ()> {
    cfg: bindings::regulator_config,
    data: T,
    regmap: Option<Arc<Regmap>>,
}

impl<T: ForeignOwnable + Send + Sync> Config<T> {
    /// Create a [`Regulator`] config.
    pub fn new(dev: &impl RawDevice, data: T) -> Self {
        Self {
            cfg: bindings::regulator_config {
                dev: dev.raw_device(),
                ..Default::default()
            },
            data,
            regmap: None,
        }
    }

    /// Assign a regmap device to the config
    #[cfg(CONFIG_REGMAP)]
    pub fn with_regmap(mut self, regmap: Arc<Regmap>) -> Self {
        self.regmap = Some(regmap);
        self
    }
}

/// Registration structure for Regulator drivers.
pub struct Registration(#[allow(dead_code)] Regulator);

impl Registration {
    /// register a Regulator driver
    pub fn register<T: ForeignOwnable + Send + Sync>(
        dev: &impl RawDevice,
        desc: &'static Desc,
        cfg: Config<T>,
    ) -> Result<Self> {
        Ok(Self(Regulator::register(dev, desc, cfg)?))
    }
}

/// Regulator device
///
/// Wraps the C structure `regulator_dev`.
///
/// # Invariants
///
/// * `self.rdev` is valid and non-null.
/// * [`Self`] has ownership of `self.rdev` memory allocation.
pub struct Regulator {
    rdev: *mut bindings::regulator_dev,
    _regmap: Option<Arc<Regmap>>,
}

impl Regulator {
    /// register a Regulator driver
    fn register<T: ForeignOwnable + Send + Sync>(
        dev: &impl RawDevice,
        desc: &'static Desc,
        mut config: Config<T>,
    ) -> Result<Self> {
        config.cfg.driver_data = config.data.into_foreign() as _;

        let regmap = config.regmap.take();
        if let Some(regmap) = &regmap {
            config.cfg.regmap = regmap.as_ptr();
        };

        // SAFETY: By the type invariants, we know that `dev.raw_device()` is always
        // valid and non-null, and the descriptor and config are guaranteed to be valid values,
        // hence it is safe to perform the FFI call.
        let rdev = from_err_ptr(unsafe {
            bindings::regulator_register(dev.raw_device(), &desc.0, &config.cfg)
        })?;

        if rdev.is_null() {
            Err(EINVAL)
        } else {
            Ok(Self {
                rdev,
                _regmap: regmap,
            })
        }
    }

    /// List voltages when the regulator is using linear mapping
    pub fn list_voltage_linear(&self, selector: u32) -> Result<i32> {
        // SAFETY: By the type invariants, we know that `self.rdev` is always valid and non-null.
        // The C function is safe to call with any selector values.
        let ret = unsafe { bindings::regulator_list_voltage_linear(self.rdev, selector) };
        if ret < 0 {
            return Err(Error::from_errno(ret));
        }
        Ok(ret)
    }

    /// Get regulator's name
    pub fn get_name(&self) -> &'static CStr {
        // SAFETY: By the type invariants, we know that `self.rdev` is always valid and non-null.
        // The C function is guaranteed to return a valid string.
        unsafe { CStr::from_char_ptr(bindings::rdev_get_name(self.rdev)) }
    }

    /// Get regulator's ID
    pub fn get_id(&self) -> i32 {
        // SAFETY: By the type invariants, we know that `self.rdev` is always valid and non-null.
        unsafe { bindings::rdev_get_id(self.rdev) }
    }

    /// Retrieve driver data associated to `self`
    pub fn data<T: ForeignOwnable>(&self) -> T::Borrowed<'_> {
        // SAFETY: By the type invariants, we know that `self.rdev` is always valid and non-null.
        unsafe { T::borrow(bindings::rdev_get_drvdata(self.rdev)) }
    }
}

impl Drop for Regulator {
    fn drop(&mut self) {
        // SAFETY: The type invariants guarantee that `self.rdev` is valid and non-null,
        // so it is safe to perform the FFI call.
        let regmap = unsafe { bindings::rdev_get_regmap(self.rdev) };

        // SAFETY: The type invariants guarantee that `self.rdev` is valid and non-null,
        // so it is safe to perform the FFI call.
        unsafe { bindings::regulator_unregister(self.rdev) }

        if !regmap.is_null() {
            drop(unsafe { Arc::from_raw(regmap) });
        }
    }
}

// SAFETY: `Regulator` has sole ownership of `self.rdev` and is never read outside of the C
// implementation. It is safe to use it from any thread.
unsafe impl Send for Regulator {}

impl crate::private::Sealed for Regulator {}

/// Helper functions to implement some of the [`Driver`] trait methods using [`Regmap`].
///
/// This trait is implemented by [`Regulator`] and is Sealed to prevent
/// to be implemented by anyone else.
#[cfg(CONFIG_REGMAP)]
pub trait RegmapHelpers: crate::private::Sealed {
    /// Implementation of [`Driver::get_voltage_sel`] using [`Regmap`].
    fn get_voltage_sel_regmap(&self) -> Result<i32>;
    /// Implementation of [`Driver::set_voltage_sel`] using [`Regmap`].
    fn set_voltage_sel_regmap(&self, sel: u32) -> Result;

    /// Implementation of [`Driver::is_enabled`] using [`Regmap`].
    ///
    /// [`Desc::with_enable`] or [`Desc::with_inverted_enable`] must have been called
    /// to setup the fields required by regmap.
    fn is_enabled_regmap(&self) -> Result<bool>;

    /// Implementation of [`Driver::enable`] using [`Regmap`].
    ///
    /// [`Desc::with_enable`] or [`Desc::with_inverted_enable`] must have been called
    /// to setup the fields required by regmap.
    fn enable_regmap(&self) -> Result;

    /// Implementation of [`Driver::disable`] using [`Regmap`].
    ///
    /// [`Desc::with_enable`] or [`Desc::with_inverted_enable`] must have been called
    /// to setup the fields required by regmap.
    fn disable_regmap(&self) -> Result;

    /// Implementation of [`Driver::set_active_discharge`] using [`Regmap`].
    ///
    /// [`Desc::with_active_discharge`] must have been called to setup the fields required
    /// by regmap.
    fn set_active_discharge_regmap(&self, enable: bool) -> Result;
    /// Implementation of [`Driver::set_current_limit`] using [`Regmap`].
    fn set_current_limit_regmap(&self, min_ua: i32, max_ua: i32) -> Result;
    /// Implementation of [`Driver::get_current_limit`] using [`Regmap`].
    fn get_current_limit_regmap(&self) -> Result<i32>;
}

#[cfg(CONFIG_REGMAP)]
impl RegmapHelpers for Regulator {
    fn get_voltage_sel_regmap(&self) -> Result<i32> {
        // SAFETY: The type invariants guarantee that `self.rdev` is valid and non-null,
        // so it is safe to perform the FFI call.
        let ret = unsafe { bindings::regulator_get_voltage_sel_regmap(self.rdev) };
        if ret < 0 {
            return Err(Error::from_errno(ret));
        }
        Ok(ret)
    }

    fn set_voltage_sel_regmap(&self, sel: u32) -> Result {
        // SAFETY: The type invariants guarantee that `self.rdev` is valid and non-null,
        // so it is safe to perform the FFI call.
        to_result(unsafe { bindings::regulator_set_voltage_sel_regmap(self.rdev, sel) })
    }

    fn is_enabled_regmap(&self) -> Result<bool> {
        // SAFETY: The type invariants guarantee that `self.rdev` is valid and non-null,
        // so it is safe to perform the FFI call.
        let ret = unsafe { bindings::regulator_is_enabled_regmap(self.rdev) };
        if ret < 0 {
            return Err(Error::from_errno(ret));
        }
        Ok(ret > 0)
    }

    fn enable_regmap(&self) -> Result {
        // SAFETY: The type invariants guarantee that `self.rdev` is valid and non-null,
        // so it is safe to perform the FFI call.
        to_result(unsafe { bindings::regulator_enable_regmap(self.rdev) })
    }

    fn disable_regmap(&self) -> Result {
        // SAFETY: The type invariants guarantee that `self.rdev` is valid and non-null,
        // so it is safe to perform the FFI call.
        to_result(unsafe { bindings::regulator_disable_regmap(self.rdev) })
    }

    fn set_active_discharge_regmap(&self, enable: bool) -> Result {
        // SAFETY: The type invariants guarantee that `self.rdev` is valid and non-null,
        // so it is safe to perform the FFI call.
        to_result(unsafe { bindings::regulator_set_active_discharge_regmap(self.rdev, enable) })
    }

    fn set_current_limit_regmap(&self, min_ua: i32, max_ua: i32) -> Result {
        // SAFETY: The type invariants guarantee that `self.rdev` is valid and non-null,
        // so it is safe to perform the FFI call.
        to_result(unsafe {
            bindings::regulator_set_current_limit_regmap(self.rdev, min_ua, max_ua)
        })
    }

    fn get_current_limit_regmap(&self) -> Result<i32> {
        // SAFETY: The type invariants guarantee that `self.rdev` is valid and non-null,
        // so it is safe to perform the FFI call.
        let ret = unsafe { bindings::regulator_get_current_limit_regmap(self.rdev) };
        if ret < 0 {
            return Err(Error::from_errno(ret));
        }
        Ok(ret)
    }
}

/// [`Regulator`] type
pub enum Type {
    /// Voltage regulator
    Voltage,
    /// Current regulator
    Current,
}

pub(crate) struct Adapter<T>(PhantomData<T>);

impl<T: Driver> Adapter<T> {
    unsafe extern "C" fn list_voltage_callback(
        rdev: *mut bindings::regulator_dev,
        selector: core::ffi::c_uint,
    ) -> core::ffi::c_int {
        let rdev = ManuallyDrop::new(Regulator {
            rdev,
            _regmap: None,
        });
        from_result(|| T::list_voltage(&rdev, selector))
    }

    unsafe extern "C" fn set_voltage_callback(
        rdev: *mut bindings::regulator_dev,
        min_uv: core::ffi::c_int,
        max_uv: core::ffi::c_int,
        selector: *mut core::ffi::c_uint,
    ) -> core::ffi::c_int {
        let rdev = ManuallyDrop::new(Regulator {
            rdev,
            _regmap: None,
        });
        match T::set_voltage(&rdev, min_uv, max_uv) {
            Ok(v) => {
                unsafe { *selector = v as _ };
                0
            }
            Err(e) => e.to_errno(),
        }
    }

    unsafe extern "C" fn map_voltage_callback(
        rdev: *mut bindings::regulator_dev,
        min_uv: core::ffi::c_int,
        max_uv: core::ffi::c_int,
    ) -> core::ffi::c_int {
        let rdev = ManuallyDrop::new(Regulator {
            rdev,
            _regmap: None,
        });
        from_result(|| T::map_voltage(&rdev, min_uv, max_uv))
    }

    unsafe extern "C" fn set_voltage_sel_callback(
        rdev: *mut bindings::regulator_dev,
        selector: core::ffi::c_uint,
    ) -> core::ffi::c_int {
        let rdev = ManuallyDrop::new(Regulator {
            rdev,
            _regmap: None,
        });
        from_result(|| {
            T::set_voltage_sel(&rdev, selector)?;
            Ok(0)
        })
    }

    unsafe extern "C" fn get_voltage_callback(
        rdev: *mut bindings::regulator_dev,
    ) -> core::ffi::c_int {
        let rdev = ManuallyDrop::new(Regulator {
            rdev,
            _regmap: None,
        });
        from_result(|| T::get_voltage(&rdev))
    }

    unsafe extern "C" fn get_voltage_sel_callback(
        rdev: *mut bindings::regulator_dev,
    ) -> core::ffi::c_int {
        let rdev = ManuallyDrop::new(Regulator {
            rdev,
            _regmap: None,
        });
        from_result(|| T::get_voltage_sel(&rdev))
    }

    unsafe extern "C" fn set_current_limit_callback(
        rdev: *mut bindings::regulator_dev,
        min_ua: core::ffi::c_int,
        max_ua: core::ffi::c_int,
    ) -> core::ffi::c_int {
        let rdev = ManuallyDrop::new(Regulator {
            rdev,
            _regmap: None,
        });
        from_result(|| {
            T::set_current_limit(&rdev, min_ua, max_ua)?;
            Ok(0)
        })
    }

    unsafe extern "C" fn get_current_limit_callback(
        rdev: *mut bindings::regulator_dev,
    ) -> core::ffi::c_int {
        let rdev = ManuallyDrop::new(Regulator {
            rdev,
            _regmap: None,
        });
        from_result(|| T::get_current_limit(&rdev))
    }

    unsafe extern "C" fn set_active_discharge_callback(
        rdev: *mut bindings::regulator_dev,
        enable: bool,
    ) -> core::ffi::c_int {
        let rdev = ManuallyDrop::new(Regulator {
            rdev,
            _regmap: None,
        });
        from_result(|| {
            T::set_active_discharge(&rdev, enable)?;
            Ok(0)
        })
    }

    unsafe extern "C" fn enable_callback(rdev: *mut bindings::regulator_dev) -> core::ffi::c_int {
        let rdev = ManuallyDrop::new(Regulator {
            rdev,
            _regmap: None,
        });
        from_result(|| {
            T::enable(&rdev)?;
            Ok(0)
        })
    }

    unsafe extern "C" fn disable_callback(rdev: *mut bindings::regulator_dev) -> core::ffi::c_int {
        let rdev = ManuallyDrop::new(Regulator {
            rdev,
            _regmap: None,
        });
        from_result(|| {
            T::disable(&rdev)?;
            Ok(0)
        })
    }

    unsafe extern "C" fn is_enabled_callback(
        rdev: *mut bindings::regulator_dev,
    ) -> core::ffi::c_int {
        let rdev = ManuallyDrop::new(Regulator {
            rdev,
            _regmap: None,
        });
        from_result(|| {
            T::is_enabled(&rdev)?;
            Ok(0)
        })
    }

    unsafe extern "C" fn set_mode_callback(
        rdev: *mut bindings::regulator_dev,
        mode: core::ffi::c_uint,
    ) -> core::ffi::c_int {
        let rdev = ManuallyDrop::new(Regulator {
            rdev,
            _regmap: None,
        });
        from_result(|| {
            let mode = Mode::try_from(mode).unwrap_or(Mode::Invalid);
            T::set_mode(&rdev, mode)?;
            Ok(0)
        })
    }

    unsafe extern "C" fn get_mode_callback(
        rdev: *mut bindings::regulator_dev,
    ) -> core::ffi::c_uint {
        let rdev = ManuallyDrop::new(Regulator {
            rdev,
            _regmap: None,
        });
        T::get_mode(&rdev) as _
    }

    unsafe extern "C" fn get_status_callback(
        rdev: *mut bindings::regulator_dev,
    ) -> core::ffi::c_int {
        let rdev = ManuallyDrop::new(Regulator {
            rdev,
            _regmap: None,
        });
        from_result(|| Ok(T::get_status(&rdev)? as _))
    }

    unsafe extern "C" fn set_suspend_voltage_callback(
        rdev: *mut bindings::regulator_dev,
        uv: core::ffi::c_int,
    ) -> core::ffi::c_int {
        let rdev = ManuallyDrop::new(Regulator {
            rdev,
            _regmap: None,
        });
        from_result(|| {
            T::set_suspend_voltage(&rdev, uv)?;
            Ok(0)
        })
    }

    unsafe extern "C" fn set_suspend_enable_callback(
        rdev: *mut bindings::regulator_dev,
    ) -> core::ffi::c_int {
        let rdev = ManuallyDrop::new(Regulator {
            rdev,
            _regmap: None,
        });
        from_result(|| {
            T::set_suspend_enable(&rdev)?;
            Ok(0)
        })
    }

    unsafe extern "C" fn set_suspend_disable_callback(
        rdev: *mut bindings::regulator_dev,
    ) -> core::ffi::c_int {
        let rdev = ManuallyDrop::new(Regulator {
            rdev,
            _regmap: None,
        });
        from_result(|| {
            T::set_suspend_disable(&rdev)?;
            Ok(0)
        })
    }

    unsafe extern "C" fn set_suspend_mode_callback(
        rdev: *mut bindings::regulator_dev,
        mode: core::ffi::c_uint,
    ) -> core::ffi::c_int {
        let rdev = ManuallyDrop::new(Regulator {
            rdev,
            _regmap: None,
        });
        from_result(|| {
            let mode = Mode::try_from(mode).unwrap_or(Mode::Invalid);
            T::set_suspend_mode(&rdev, mode)?;
            Ok(0)
        })
    }

    const VTABLE: bindings::regulator_ops = bindings::regulator_ops {
        list_voltage: if T::HAS_LIST_VOLTAGE {
            Some(Adapter::<T>::list_voltage_callback)
        } else {
            None
        },
        set_voltage: if T::HAS_SET_VOLTAGE {
            Some(Adapter::<T>::set_voltage_callback)
        } else {
            None
        },
        map_voltage: if T::HAS_MAP_VOLTAGE {
            Some(Adapter::<T>::map_voltage_callback)
        } else {
            None
        },
        set_voltage_sel: if T::HAS_SET_VOLTAGE_SEL {
            Some(Adapter::<T>::set_voltage_sel_callback)
        } else {
            None
        },
        get_voltage: if T::HAS_GET_VOLTAGE {
            Some(Adapter::<T>::get_voltage_callback)
        } else {
            None
        },
        get_voltage_sel: if T::HAS_GET_VOLTAGE_SEL {
            Some(Adapter::<T>::get_voltage_sel_callback)
        } else {
            None
        },
        set_current_limit: if T::HAS_SET_CURRENT_LIMIT {
            Some(Adapter::<T>::set_current_limit_callback)
        } else {
            None
        },
        get_current_limit: if T::HAS_GET_CURRENT_LIMIT {
            Some(Adapter::<T>::get_current_limit_callback)
        } else {
            None
        },
        set_active_discharge: if T::HAS_SET_ACTIVE_DISCHARGE {
            Some(Adapter::<T>::set_active_discharge_callback)
        } else {
            None
        },
        enable: if T::HAS_ENABLE {
            Some(Adapter::<T>::enable_callback)
        } else {
            None
        },
        disable: if T::HAS_DISABLE {
            Some(Adapter::<T>::disable_callback)
        } else {
            None
        },
        is_enabled: if T::HAS_IS_ENABLED {
            Some(Adapter::<T>::is_enabled_callback)
        } else {
            None
        },
        set_mode: if T::HAS_SET_MODE {
            Some(Adapter::<T>::set_mode_callback)
        } else {
            None
        },
        get_mode: if T::HAS_GET_MODE {
            Some(Adapter::<T>::get_mode_callback)
        } else {
            None
        },
        get_status: if T::HAS_GET_STATUS {
            Some(Adapter::<T>::get_status_callback)
        } else {
            None
        },
        set_suspend_voltage: if T::HAS_SET_SUSPEND_VOLTAGE {
            Some(Adapter::<T>::set_suspend_voltage_callback)
        } else {
            None
        },
        set_suspend_enable: if T::HAS_SET_SUSPEND_ENABLE {
            Some(Adapter::<T>::set_suspend_enable_callback)
        } else {
            None
        },
        set_suspend_disable: if T::HAS_SET_SUSPEND_DISABLE {
            Some(Adapter::<T>::set_suspend_disable_callback)
        } else {
            None
        },
        set_suspend_mode: if T::HAS_SET_SUSPEND_MODE {
            Some(Adapter::<T>::set_suspend_mode_callback)
        } else {
            None
        },
        // SAFETY: The rest is zeroed out to initialize `struct regulator_ops`,
        // sets `Option<&F>` to be `None`.
        ..unsafe { core::mem::MaybeUninit::<bindings::regulator_ops>::zeroed().assume_init() }
    };

    const fn build() -> &'static bindings::regulator_ops {
        &Self::VTABLE
    }
}
