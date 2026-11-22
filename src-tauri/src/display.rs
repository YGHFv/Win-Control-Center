use serde::{Deserialize, Serialize};
use std::mem::size_of;
use std::os::windows::process::CommandExt;
use std::process::Command;
use std::time::{Duration, Instant};
use windows::core::PCWSTR;
use windows::Win32::Devices::Display::{
    DestroyPhysicalMonitor, GetMonitorBrightness, GetNumberOfPhysicalMonitorsFromHMONITOR,
    GetPhysicalMonitorsFromHMONITOR, GetVCPFeatureAndVCPFeatureReply, SetMonitorBrightness,
    SetVCPFeature, PHYSICAL_MONITOR,
};
use windows::Win32::Foundation::{BOOL, LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayDevicesW, EnumDisplayMonitors, GetMonitorInfoW, DISPLAY_DEVICEW,
    DISPLAY_DEVICE_ACTIVE, HDC, HMONITOR, MONITORINFO, MONITORINFOEXW,
};
use windows::Win32::UI::WindowsAndMessaging::EDD_GET_DEVICE_INTERFACE_NAME;

const CREATE_NO_WINDOW: u32 = 0x08000000;
const CACHE_DURATION: Duration = Duration::from_secs(5);
const VCP_BRIGHTNESS: u8 = 0x10;

#[derive(Clone, Debug, Serialize)]
pub struct MonitorBrightness {
    pub id: String,
    pub number: u32,
    pub name: String,
    pub brightness: Option<f32>,
}

#[derive(Default)]
pub struct BrightnessCache {
    monitors: Vec<MonitorBrightness>,
    last_fetch: Option<Instant>,
}

// Serialize reads and writes so an old hardware read cannot overwrite a slider change.
pub type BrightnessState = tokio::sync::Mutex<BrightnessCache>;

struct PhysicalMonitors(Vec<PHYSICAL_MONITOR>);

impl Drop for PhysicalMonitors {
    fn drop(&mut self) {
        for monitor in &self.0 {
            unsafe {
                let _ = DestroyPhysicalMonitor(monitor.hPhysicalMonitor);
            }
        }
    }
}

struct Monitor {
    info: MonitorBrightness,
    instance_id: String,
    physical: PhysicalMonitors,
    physical_index: usize,
}

fn wide_string(value: &[u16]) -> String {
    let end = value.iter().position(|&c| c == 0).unwrap_or(value.len());
    String::from_utf16_lossy(&value[..end])
}

fn normalize_instance_id(value: &str) -> String {
    let value = value.trim().to_ascii_lowercase();
    let value = value.strip_prefix(r"\\?\").unwrap_or(&value);
    let value = value.split("#{").next().unwrap_or(value).replace('#', r"\");
    // WMI adds an instance suffix (usually "_0") to the monitor device path.
    match value.rsplit_once('_') {
        Some((base, suffix))
            if !suffix.is_empty() && suffix.bytes().all(|c| c.is_ascii_digit()) =>
        {
            base.to_string()
        }
        _ => value,
    }
}

fn display_number(device_name: &str) -> u32 {
    device_name
        .strip_prefix(r"\\.\DISPLAY")
        .and_then(|number| number.parse().ok())
        .unwrap_or(u32::MAX)
}

unsafe extern "system" fn collect_monitor(
    monitor: HMONITOR,
    _hdc: HDC,
    _rect: *mut RECT,
    data: LPARAM,
) -> BOOL {
    let monitors = &mut *(data.0 as *mut Vec<HMONITOR>);
    monitors.push(monitor);
    BOOL(1)
}

fn enumerate_monitors() -> Result<Vec<Monitor>, String> {
    let mut handles = Vec::<HMONITOR>::new();
    unsafe {
        EnumDisplayMonitors(
            HDC::default(),
            None,
            Some(collect_monitor),
            LPARAM(&mut handles as *mut _ as isize),
        )
        .ok()
        .map_err(|e| e.to_string())?;
    }

    let mut monitors = Vec::new();
    for handle in handles {
        let mut info = MONITORINFOEXW::default();
        info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
        if !unsafe { GetMonitorInfoW(handle, &mut info as *mut _ as *mut MONITORINFO) }.as_bool() {
            continue;
        }
        let device_name = wide_string(&info.szDevice);
        let number = display_number(&device_name);
        let mut devices = Vec::new();
        for index in 0.. {
            let mut device = DISPLAY_DEVICEW::default();
            device.cb = size_of::<DISPLAY_DEVICEW>() as u32;
            if !unsafe {
                EnumDisplayDevicesW(
                    PCWSTR(info.szDevice.as_ptr()),
                    index,
                    &mut device,
                    EDD_GET_DEVICE_INTERFACE_NAME,
                )
            }
            .as_bool()
            {
                break;
            }
            if device.StateFlags & DISPLAY_DEVICE_ACTIVE != 0 {
                devices.push(device);
            }
        }

        let mut count = 0;
        let mut physical = Vec::new();
        unsafe {
            if GetNumberOfPhysicalMonitorsFromHMONITOR(handle, &mut count).is_ok() && count > 0 {
                physical.resize(count as usize, PHYSICAL_MONITOR::default());
                if GetPhysicalMonitorsFromHMONITOR(handle, &mut physical).is_err() {
                    physical.clear();
                }
            }
        }
        let mut physical = PhysicalMonitors(physical);
        // The APIs only allow association by index. A mismatch can occur during
        // hotplug, so do not assign a handle to a different display by guessing.
        let can_associate =
            devices.len() == physical.0.len() || (devices.is_empty() && physical.0.len() == 1);
        // A cloned desktop can expose several physical monitors under one logical display.
        let row_count = if devices.is_empty() {
            physical.0.len().max(1)
        } else {
            devices.len()
        };
        for index in (0..row_count).rev() {
            let device = devices.get(index);
            let instance_id = device
                .map(|device| normalize_instance_id(&wide_string(&device.DeviceID)))
                .unwrap_or_default();
            let physical_monitor = if can_associate && index < physical.0.len() {
                vec![physical.0.remove(index)]
            } else {
                Vec::new()
            };
            let description = device
                .map(|device| wide_string(&device.DeviceString))
                .filter(|name| !name.is_empty())
                .or_else(|| {
                    physical_monitor.first().map(|p| {
                        let description = p.szPhysicalMonitorDescription;
                        wide_string(&description)
                    })
                })
                .unwrap_or_else(|| format!("显示器 {}", number));
            let id = if instance_id.is_empty() {
                format!("{}:{}", device_name, index)
            } else {
                instance_id.clone()
            };
            monitors.push(Monitor {
                info: MonitorBrightness {
                    id,
                    number,
                    name: description,
                    brightness: None,
                },
                instance_id,
                physical: PhysicalMonitors(physical_monitor),
                physical_index: index,
            });
        }
    }
    monitors.sort_by_key(|monitor| (monitor.info.number, monitor.physical_index));
    Ok(monitors)
}

fn read_ddc_brightness(monitor: &Monitor) -> Option<f32> {
    let handle = monitor.physical.0.first()?.hPhysicalMonitor;
    let (mut min, mut current, mut max) = (0, 0, 0);
    unsafe {
        if BOOL(GetMonitorBrightness(
            handle,
            &mut min,
            &mut current,
            &mut max,
        ))
        .as_bool()
            && max > min
        {
            return Some((current.saturating_sub(min) as f32 / (max - min) as f32).clamp(0.0, 1.0));
        }
        // Some monitors only implement the MCCS luminance feature.
        if BOOL(GetVCPFeatureAndVCPFeatureReply(
            handle,
            VCP_BRIGHTNESS,
            None,
            &mut current,
            Some(&mut max),
        ))
        .as_bool()
            && max > 0
        {
            return Some((current as f32 / max as f32).clamp(0.0, 1.0));
        }
    }
    None
}

fn set_ddc_brightness(monitor: &Monitor, value: f32) -> Result<(), String> {
    let handle = monitor
        .physical
        .0
        .first()
        .ok_or("显示器没有可用的 DDC/CI 接口")?
        .hPhysicalMonitor;
    let (mut min, mut current, mut max) = (0, 0, 0);
    unsafe {
        if BOOL(GetMonitorBrightness(
            handle,
            &mut min,
            &mut current,
            &mut max,
        ))
        .as_bool()
            && max > min
        {
            let target = min + (value * (max - min) as f32).round() as u32;
            if BOOL(SetMonitorBrightness(handle, target)).as_bool() {
                return Ok(());
            }
        }
        BOOL(GetVCPFeatureAndVCPFeatureReply(
            handle,
            VCP_BRIGHTNESS,
            None,
            &mut current,
            Some(&mut max),
        ))
        .ok()
        .map_err(|e| e.to_string())?;
        if max == 0 {
            return Err("显示器返回了无效的亮度范围".into());
        }
        BOOL(SetVCPFeature(
            handle,
            VCP_BRIGHTNESS,
            (value * max as f32).round() as u32,
        ))
        .ok()
        .map_err(|e| e.to_string())
    }
}

#[derive(Deserialize)]
struct WmiMonitor {
    instance_name: String,
    name: Option<String>,
    brightness: Option<u32>,
}

fn powershell(script: &str) -> Result<String, String> {
    let script = format!(
        "[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false); $OutputEncoding = [System.Text.UTF8Encoding]::new($false); {}",
        script
    );
    let output = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command"])
        .arg(script)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| e.to_string())?;
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

fn read_wmi_monitors() -> Result<Vec<WmiMonitor>, String> {
    let json = powershell(
        r#"[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$brightness = @(Get-CimInstance -Namespace root/wmi -ClassName WmiMonitorBrightness -ErrorAction SilentlyContinue)
$names = @(Get-CimInstance -Namespace root/wmi -ClassName WmiMonitorID -ErrorAction SilentlyContinue)
$instances = @(@($names | Where-Object Active | ForEach-Object InstanceName) + @($brightness | Where-Object Active | ForEach-Object InstanceName) | Sort-Object -Unique)
$rows = @($instances | ForEach-Object {
    $instance = $_
    $monitor = $names | Where-Object InstanceName -EQ $instance | Select-Object -First 1
    $level = $brightness | Where-Object InstanceName -EQ $instance | Select-Object -First 1
    $name = if ($monitor) { -join ($monitor.UserFriendlyName | Where-Object { $_ -ne 0 } | ForEach-Object { [char]$_ }) } else { $null }
    $value = if ($level) { [int]$level.CurrentBrightness } else { $null }
    @{ instance_name = $instance; name = $name; brightness = $value }
})
ConvertTo-Json -InputObject $rows -Compress"#,
    )?;
    serde_json::from_str(&json).map_err(|e| e.to_string())
}

fn read_monitors() -> Result<Vec<MonitorBrightness>, String> {
    let mut monitors = enumerate_monitors()?;
    for monitor in &mut monitors {
        monitor.info.brightness = read_ddc_brightness(monitor);
    }
    if !monitors.is_empty() {
        if let Ok(wmi_monitors) = read_wmi_monitors() {
            for monitor in &mut monitors {
                if let Some(wmi) = wmi_monitors
                    .iter()
                    .find(|wmi| normalize_instance_id(&wmi.instance_name) == monitor.instance_id)
                {
                    if monitor.info.brightness.is_none() {
                        monitor.info.brightness =
                            wmi.brightness.map(|value| value.min(100) as f32 / 100.0);
                    }
                    if let Some(name) = wmi.name.as_ref().filter(|name| !name.is_empty()) {
                        monitor.info.name = name.clone();
                    }
                }
            }
        }
    }
    Ok(monitors.into_iter().map(|monitor| monitor.info).collect())
}

fn write_monitor(id: &str, value: f32) -> Result<(), String> {
    let monitors = enumerate_monitors()?;
    let monitor = monitors
        .iter()
        .find(|monitor| monitor.info.id == id)
        .ok_or("显示器已断开，请刷新后重试")?;
    if set_ddc_brightness(monitor, value).is_ok() {
        return Ok(());
    }
    if monitor.instance_id.is_empty() {
        return Err("此显示器不支持亮度调节，请检查 DDC/CI 设置".into());
    }
    let wmi = read_wmi_monitors()?
        .into_iter()
        .find(|wmi| {
            normalize_instance_id(&wmi.instance_name) == monitor.instance_id
                && wmi.brightness.is_some()
        })
        .ok_or("此显示器不支持亮度调节，请检查 DDC/CI 设置")?;
    // Match one exact WMI instance; never fall back to writing every monitor.
    let instance = wmi.instance_name.replace('\'', "''");
    let script = format!(
        "$ErrorActionPreference = 'Stop'; \
         $monitor = Get-CimInstance -Namespace root/wmi -ClassName WmiMonitorBrightnessMethods \
         | Where-Object InstanceName -EQ '{}'; \
         if (-not $monitor) {{ throw 'Monitor disconnected' }}; \
         $result = Invoke-CimMethod -InputObject $monitor -MethodName WmiSetBrightness \
         -Arguments @{{ Timeout = [uint32]1; Brightness = [byte]{} }}; \
         if ($null -ne $result.ReturnValue -and [int]$result.ReturnValue -ne 0) {{ throw 'Brightness update failed' }}",
        instance,
        (value * 100.0).round() as u32
    );
    powershell(&script).map(|_| ())
}

pub async fn get_monitor_brightness(
    state: &BrightnessState,
) -> Result<Vec<MonitorBrightness>, String> {
    let mut cache = state.lock().await;
    if cache
        .last_fetch
        .is_some_and(|last| last.elapsed() < CACHE_DURATION)
    {
        return Ok(cache.monitors.clone());
    }
    let monitors = tokio::task::spawn_blocking(read_monitors)
        .await
        .map_err(|e| e.to_string())??;
    cache.monitors = monitors;
    cache.last_fetch = Some(Instant::now());
    Ok(cache.monitors.clone())
}

pub async fn set_monitor_brightness(
    state: &BrightnessState,
    id: String,
    value: f32,
) -> Result<(), String> {
    if !value.is_finite() || !(0.0..=1.0).contains(&value) {
        return Err("亮度必须在 0% 到 100% 之间".into());
    }
    let mut cache = state.lock().await;
    let target_id = id.clone();
    let result = tokio::task::spawn_blocking(move || write_monitor(&target_id, value))
        .await
        .map_err(|e| e.to_string())?;
    if result.is_ok() {
        if let Some(monitor) = cache.monitors.iter_mut().find(|monitor| monitor.id == id) {
            monitor.brightness = Some(value);
        }
    } else {
        cache.last_fetch = None;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monitor_device_path_matches_exact_wmi_instance() {
        assert_eq!(
            normalize_instance_id(r"\\?\DISPLAY#DEL1234#5&abc&0&UID100#{guid}"),
            normalize_instance_id(r"DISPLAY\DEL1234\5&abc&0&UID100_0"),
        );
        assert_ne!(
            normalize_instance_id(r"DISPLAY\DEL1234\5&abc&0&UID100_0"),
            normalize_instance_id(r"DISPLAY\DEL1234\5&abc&0&UID101_0"),
        );
    }

    #[test]
    fn monitor_number_uses_windows_display_number() {
        assert_eq!(display_number(r"\\.\DISPLAY2"), 2);
        assert_eq!(display_number(r"\\.\DISPLAY12"), 12);
    }
}
