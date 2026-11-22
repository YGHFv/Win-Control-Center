use super::{normalize_instance_id, wide_string};
use std::ptr::null_mut;
use windows::core::{w, BSTR, PCWSTR};
use windows::Win32::Foundation::{ERROR_TIMEOUT, E_UNEXPECTED, RPC_E_CHANGED_MODE};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoSetProxyBlanket, CoTaskMemFree, CoUninitialize,
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, EOAC_NONE, RPC_C_AUTHN_LEVEL_CALL,
    RPC_C_IMP_LEVEL_IMPERSONATE,
};
use windows::Win32::System::Rpc::{RPC_C_AUTHN_WINNT, RPC_C_AUTHZ_NONE};
use windows::Win32::System::Variant::{
    VariantClear, VariantToUInt16ArrayAlloc, VariantToUInt32, VARIANT, VT_BSTR, VT_EMPTY, VT_I4,
    VT_NULL, VT_UI1,
};
use windows::Win32::System::Wmi::{
    IWbemClassObject, IWbemLocator, IWbemServices, WbemLocator, WBEM_E_INVALID_CLASS,
    WBEM_E_NOT_FOUND, WBEM_FLAG_BIDIRECTIONAL, WBEM_FLAG_CONNECT_USE_MAX_WAIT,
    WBEM_FLAG_FORWARD_ONLY, WBEM_FLAG_RETURN_IMMEDIATELY, WBEM_S_TIMEDOUT,
};

struct ComApartment(bool);

impl Drop for ComApartment {
    fn drop(&mut self) {
        if self.0 {
            unsafe { CoUninitialize() };
        }
    }
}

#[derive(Default)]
struct Property(VARIANT);

impl Drop for Property {
    fn drop(&mut self) {
        let _ = unsafe { VariantClear(&mut self.0) };
    }
}

impl Property {
    fn get(object: &IWbemClassObject, name: PCWSTR) -> windows::core::Result<Self> {
        let mut property = Self::default();
        unsafe { object.Get(name, 0, &mut property.0, None, None)? };
        Ok(property)
    }

    fn string(&self) -> Option<String> {
        unsafe {
            let value = &*self.0.Anonymous.Anonymous;
            (value.vt == VT_BSTR).then(|| value.Anonymous.bstrVal.to_string())
        }
    }

    fn utf16_array(&self) -> Option<String> {
        let mut data = null_mut();
        let mut length = 0;
        unsafe {
            VariantToUInt16ArrayAlloc(&self.0, &mut data, &mut length).ok()?;
            let text = if length == 0 || data.is_null() {
                String::new()
            } else {
                wide_string(std::slice::from_raw_parts(data, length as usize))
            };
            CoTaskMemFree(Some(data.cast()));
            Some(text)
        }
    }
}

fn with_services<T>(run: impl FnOnce(&IWbemServices) -> Result<T, String>) -> Result<T, String> {
    unsafe {
        let _apartment = match CoInitializeEx(None, COINIT_MULTITHREADED) {
            Ok(()) => ComApartment(true),
            Err(error) if error.code() == RPC_E_CHANGED_MODE => ComApartment(false),
            Err(error) => return Err(error.to_string()),
        };
        let locator: IWbemLocator = CoCreateInstance(&WbemLocator, None, CLSCTX_INPROC_SERVER)
            .map_err(|error| error.to_string())?;
        let services = locator
            .ConnectServer(
                &BSTR::from(r"ROOT\WMI"),
                None,
                None,
                None,
                WBEM_FLAG_CONNECT_USE_MAX_WAIT.0,
                None,
                None,
            )
            .map_err(|error| error.to_string())?;
        CoSetProxyBlanket(
            &services,
            RPC_C_AUTHN_WINNT,
            RPC_C_AUTHZ_NONE,
            PCWSTR::null(),
            RPC_C_AUTHN_LEVEL_CALL,
            RPC_C_IMP_LEVEL_IMPERSONATE,
            None,
            EOAC_NONE,
        )
        .map_err(|error| error.to_string())?;
        run(&services)
    }
}

fn query(services: &IWbemServices, query: &str) -> windows::core::Result<Vec<IWbemClassObject>> {
    unsafe {
        let rows = services.ExecQuery(
            &BSTR::from("WQL"),
            &BSTR::from(query),
            WBEM_FLAG_FORWARD_ONLY | WBEM_FLAG_RETURN_IMMEDIATELY,
            None,
        )?;
        let mut result = Vec::new();
        loop {
            let mut row = [None];
            let mut count = 0;
            let status = rows.Next(1000, &mut row, &mut count);
            status.ok()?;
            if status.0 == WBEM_S_TIMEDOUT.0 {
                return Err(ERROR_TIMEOUT.to_hresult().into());
            }
            match row[0].take() {
                Some(object) => result.push(object),
                None => return Ok(result),
            }
        }
    }
}

#[derive(Default)]
pub(super) struct MonitorData {
    pub(super) name: Option<String>,
    pub(super) brightness: Option<f32>,
}

pub(super) fn read_monitors() -> std::collections::HashMap<String, MonitorData> {
    with_services(|services| {
        let mut monitors = std::collections::HashMap::<_, MonitorData>::new();
        for row in query(
            services,
            "SELECT InstanceName, CurrentBrightness FROM WmiMonitorBrightness WHERE Active = TRUE",
        )
        .unwrap_or_default()
        {
            if let Some(id) = instance_id(&row) {
                if let Ok(property) = Property::get(&row, w!("CurrentBrightness")) {
                    if let Ok(value) = unsafe { VariantToUInt32(&property.0) } {
                        monitors.entry(id).or_default().brightness =
                            Some(value.min(100) as f32 / 100.0);
                    }
                }
            }
        }
        for row in query(
            services,
            "SELECT InstanceName, UserFriendlyName FROM WmiMonitorID WHERE Active = TRUE",
        )
        .unwrap_or_default()
        {
            if let Some(id) = instance_id(&row) {
                monitors.entry(id).or_default().name = Property::get(&row, w!("UserFriendlyName"))
                    .ok()
                    .and_then(|property| property.utf16_array())
                    .filter(|name| !name.is_empty());
            }
        }
        Ok(monitors)
    })
    .unwrap_or_default()
}

fn instance_id(row: &IWbemClassObject) -> Option<String> {
    Property::get(row, w!("InstanceName"))
        .ok()?
        .string()
        .map(|name| normalize_instance_id(&name))
}

pub(super) fn set_brightness(id: &str, value: f32) -> Result<bool, String> {
    with_services(|services| {
        let result = (|| unsafe {
            let rows = match query(
                services,
                "SELECT * FROM WmiMonitorBrightnessMethods WHERE Active = TRUE",
            ) {
                Ok(rows) => rows,
                Err(error) if error.code().0 == WBEM_E_INVALID_CLASS.0 => return Ok(None),
                Err(error) => return Err(error),
            };
            let mut matches = rows
                .iter()
                .filter(|row| instance_id(row).as_deref() == Some(id));
            let Some(row) = matches.next().filter(|_| matches.next().is_none()) else {
                return Ok(None);
            };
            let path = Property::get(row, w!("__PATH"))?
                .string()
                .ok_or(E_UNEXPECTED)?;
            let mut class = None;
            services.GetObject(
                &BSTR::from("WmiMonitorBrightnessMethods"),
                WBEM_FLAG_BIDIRECTIONAL,
                None,
                Some(&mut class),
                None,
            )?;
            let mut signature = None;
            class.ok_or(E_UNEXPECTED)?.GetMethod(
                w!("WmiSetBrightness"),
                0,
                &mut signature,
                null_mut(),
            )?;
            let params = signature.ok_or(E_UNEXPECTED)?.SpawnInstance(0)?;
            let mut timeout = Property::default();
            let timeout_value = &mut *timeout.0.Anonymous.Anonymous;
            timeout_value.vt = VT_I4;
            timeout_value.Anonymous.lVal = 0;
            params.Put(w!("Timeout"), 0, &timeout.0, 0)?;
            let mut brightness = Property::default();
            let brightness_value = &mut *brightness.0.Anonymous.Anonymous;
            brightness_value.vt = VT_UI1;
            brightness_value.Anonymous.bVal = (value * 100.0).round() as u8;
            params.Put(w!("Brightness"), 0, &brightness.0, 0)?;
            let mut output = None;
            services.ExecMethod(
                &BSTR::from(path),
                &BSTR::from("WmiSetBrightness"),
                WBEM_FLAG_BIDIRECTIONAL,
                None,
                &params,
                Some(&mut output),
                None,
            )?;
            let Some(output) = output else {
                return Ok(Some(0));
            };
            match Property::get(&output, w!("ReturnValue")) {
                Ok(property) => match property.0.Anonymous.Anonymous.vt {
                    VT_EMPTY | VT_NULL => Ok(Some(0)),
                    _ => VariantToUInt32(&property.0).map(Some),
                },
                Err(error) if error.code().0 == WBEM_E_NOT_FOUND.0 => Ok(Some(0)),
                Err(error) => Err(error),
            }
        })()
        .map_err(|error| format!("WMI 亮度调节失败：{error}"))?;
        match result {
            Some(0) => Ok(true),
            Some(code) => Err(format!("WMI 亮度调节失败（返回码 {code}）")),
            None => Ok(false),
        }
    })
}
