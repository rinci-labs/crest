//! Set the laptop screen brightness via WMI (class `WmiMonitorBrightnessMethods`), the same way
//! the Windows brightness slider does. External monitors are not supported; their brightness is
//! set with the monitor's own buttons.

use windows::Win32::Foundation::E_FAIL;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, CoCreateInstance, CoSetProxyBlanket, EOAC_NONE, RPC_C_AUTHN_LEVEL_CALL,
    RPC_C_IMP_LEVEL_IMPERSONATE,
};
use windows::Win32::System::Rpc::{RPC_C_AUTHN_WINNT, RPC_C_AUTHZ_NONE};
use windows::Win32::System::Variant::VARIANT;
use windows::Win32::System::Wmi::{
    IWbemClassObject, IWbemLocator, IWbemServices, WBEM_FLAG_FORWARD_ONLY, WBEM_GENERIC_FLAG_TYPE, WBEM_INFINITE,
    WbemLocator,
};
use windows::core::{BSTR, Error, PCWSTR, Result, w};

pub struct Brightness {
    services: IWbemServices,
    /// WMI path of the internal screen object.
    path: BSTR,
    /// `WmiSetBrightness` parameter template, filled in fresh on each use.
    input: IWbemClassObject,
}

impl Brightness {
    /// Fails if the screen does not support brightness control from Windows.
    pub fn new() -> Result<Self> {
        let missing = || Error::from(E_FAIL);
        unsafe {
            let locator: IWbemLocator = CoCreateInstance(&WbemLocator, None, CLSCTX_INPROC_SERVER)?;
            let empty = BSTR::new();
            let services = locator.ConnectServer(&BSTR::from("ROOT\\WMI"), &empty, &empty, &empty, 0, &empty, None)?;
            CoSetProxyBlanket(
                &services,
                RPC_C_AUTHN_WINNT,
                RPC_C_AUTHZ_NONE,
                PCWSTR::null(),
                RPC_C_AUTHN_LEVEL_CALL,
                RPC_C_IMP_LEVEL_IMPERSONATE,
                None,
                EOAC_NONE,
            )?;
            let query = BSTR::from("SELECT * FROM WmiMonitorBrightnessMethods");
            let found = services.ExecQuery(&BSTR::from("WQL"), &query, WBEM_FLAG_FORWARD_ONLY, None)?;
            let (mut objects, mut count) = ([None], 0u32);
            found.Next(WBEM_INFINITE, &mut objects, &mut count).ok()?;
            let screen = objects[0].take().filter(|_| count == 1).ok_or_else(missing)?;
            let mut path = VARIANT::default();
            screen.Get(w!("__PATH"), 0, &mut path, None, None)?;
            let path = BSTR::try_from(&path)?;

            let mut class = None;
            let name = BSTR::from("WmiMonitorBrightnessMethods");
            services.GetObject(&name, WBEM_GENERIC_FLAG_TYPE(0), None, Some(&mut class), None)?;
            let mut input = None;
            class.ok_or_else(missing)?.GetMethod(w!("WmiSetBrightness"), 0, &mut input, std::ptr::null_mut())?;
            Ok(Self { services, path, input: input.ok_or_else(missing)? })
        }
    }

    /// Set brightness to `percent` (0..100) immediately.
    pub fn set(&self, percent: u8) -> Result<()> {
        unsafe {
            let params = self.input.SpawnInstance(0)?;
            params.Put(w!("Timeout"), 0, &VARIANT::from(0i32), 0)?;
            params.Put(w!("Brightness"), 0, &VARIANT::from(percent.min(100)), 0)?;
            let method = BSTR::from("WmiSetBrightness");
            self.services.ExecMethod(&self.path, &method, WBEM_GENERIC_FLAG_TYPE(0), None, &params, None, None)
        }
    }
}
