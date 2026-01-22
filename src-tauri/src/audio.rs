#![allow(non_snake_case)]
#![allow(non_camel_case_types)]
#![allow(unused_imports)]
#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::mpsc::{channel, Sender};
use std::sync::Mutex;
use std::thread;
use windows::core::{interface, ComInterface, IUnknown, Result, GUID, HRESULT, PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, E_NOINTERFACE};

use base64::{engine::general_purpose, Engine as _};
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, GetObjectW, SelectObject, BITMAP,
    BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS,
};
use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::*;
use windows::Win32::System::Com::StructuredStorage::{PropVariantClear, PROPVARIANT};
use windows::Win32::System::Com::*;
use windows::Win32::System::Diagnostics::ToolHelp::*;
use windows::Win32::System::ProcessStatus::GetModuleBaseNameW;
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_INFORMATION,
    PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ,
};
use windows::Win32::UI::Shell::ExtractIconExW;
use windows::Win32::UI::WindowsAndMessaging::{
    DestroyIcon, GetIconInfo, PrivateExtractIconsW, HICON, ICONINFO,
};

use tokio::sync::oneshot;

// Modern Client (Win 10/11)
const CLSID_POLICY_CONFIG_CLIENT: GUID = GUID {
    data1: 0x870af99c,
    data2: 0x171d,
    data3: 0x4f9e,
    data4: [0xaf, 0x0d, 0xe6, 0x3d, 0xf4, 0x0c, 0x2b, 0xc9],
};

// Vista Client (Classic, often more stable on Win11 for simple switching)
const CLSID_POLICY_CONFIG_VISTA: GUID = GUID {
    data1: 0x294935ce,
    data2: 0xf637,
    data3: 0x4e7c,
    data4: [0xa4, 0x1b, 0xab, 0x25, 0x54, 0x60, 0xf8, 0x62],
};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AppVolume {
    pub pid: u32,
    pub name: String,
    pub volume: f32,
    pub is_muted: bool,
    pub icon_path: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct AudioDevice {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

pub enum AudioRequest {
    GetMasterVolume(oneshot::Sender<Result<f32>>),
    GetMicVolume(oneshot::Sender<Result<f32>>),
    GetAppVolumes(oneshot::Sender<Result<Vec<AppVolume>>>),
    SetMasterVolume(f32),
    SetMicVolume(f32),
    SetAppVolume(u32, f32),
    SetAppMute(u32, bool),
    GetPlaybackDevices(oneshot::Sender<Result<Vec<AudioDevice>>>),
    GetCaptureDevices(oneshot::Sender<Result<Vec<AudioDevice>>>),
    SetDefaultDevice(String),
}

pub struct AppCache {
    pub names: Mutex<HashMap<u32, (String, String)>>,
}

impl AppCache {
    pub fn new() -> Self {
        Self {
            names: Mutex::new(HashMap::new()),
        }
    }
}

pub struct AudioState {
    pub tx: Sender<AudioRequest>,
}

impl AudioState {
    pub fn new(cache: std::sync::Arc<AppCache>) -> Self {
        let (tx, rx) = channel::<AudioRequest>();
        let worker_cache = cache.clone();

        thread::spawn(move || {
            unsafe {
                let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            }
            struct AudioContext {
                enumerator: IMMDeviceEnumerator,
            }
            impl AudioContext {
                unsafe fn new() -> Result<Self> {
                    let enumerator: IMMDeviceEnumerator =
                        CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
                    Ok(Self { enumerator })
                }
                unsafe fn get_sys(&mut self) -> Result<IAudioEndpointVolume> {
                    let device = self
                        .enumerator
                        .GetDefaultAudioEndpoint(eRender, eMultimedia)?;
                    device.Activate(CLSCTX_ALL, None::<*const PROPVARIANT>)
                }
                unsafe fn get_mic(&mut self) -> Result<IAudioEndpointVolume> {
                    let device = self
                        .enumerator
                        .GetDefaultAudioEndpoint(eCapture, eMultimedia)?;
                    device.Activate(CLSCTX_ALL, None::<*const PROPVARIANT>)
                }
            }
            let mut ctx = unsafe { AudioContext::new().ok() };
            while let Ok(req) = rx.recv() {
                if ctx.is_none() {
                    ctx = unsafe { AudioContext::new().ok() };
                }
                if let Some(ref mut c) = ctx {
                    match req {
                        AudioRequest::GetMasterVolume(res_tx) => {
                            let res =
                                unsafe { c.get_sys().and_then(|v| v.GetMasterVolumeLevelScalar()) };
                            let _ = res_tx.send(res);
                        }
                        AudioRequest::GetMicVolume(res_tx) => {
                            let res =
                                unsafe { c.get_mic().and_then(|v| v.GetMasterVolumeLevelScalar()) };
                            let _ = res_tx.send(res);
                        }
                        AudioRequest::GetAppVolumes(res_tx) => {
                            let res =
                                unsafe { internal_get_app_volumes(&c.enumerator, &worker_cache) };
                            let _ = res_tx.send(res);
                        }
                        AudioRequest::SetMasterVolume(vol) => {
                            if let Ok(v) = unsafe { c.get_sys() } {
                                let _ =
                                    unsafe { v.SetMasterVolumeLevelScalar(vol, std::ptr::null()) };
                            }
                        }
                        AudioRequest::SetMicVolume(vol) => {
                            if let Ok(v) = unsafe { c.get_mic() } {
                                let _ =
                                    unsafe { v.SetMasterVolumeLevelScalar(vol, std::ptr::null()) };
                            }
                        }
                        AudioRequest::SetAppVolume(pid, vol) => {
                            let _ = unsafe { internal_set_app_vol(&c.enumerator, pid, vol) };
                        }
                        AudioRequest::SetAppMute(pid, mute) => {
                            let _ = unsafe { internal_set_app_mute(&c.enumerator, pid, mute) };
                        }
                        AudioRequest::GetPlaybackDevices(tx) => {
                            let res = unsafe { get_audio_endpoints(&c.enumerator, eRender) };
                            let _ = tx.send(res);
                        }
                        AudioRequest::GetCaptureDevices(tx) => {
                            let res = unsafe { get_audio_endpoints(&c.enumerator, eCapture) };
                            let _ = tx.send(res);
                        }
                        AudioRequest::SetDefaultDevice(id) => {
                            let _ = unsafe { set_default_device(&id) };
                        }
                    }
                }
            }
        });
        Self { tx }
    }
}

unsafe fn internal_get_app_volumes(
    enumerator: &IMMDeviceEnumerator,
    cache: &AppCache,
) -> Result<Vec<AppVolume>> {
    let mut session_map: HashMap<u32, (f32, bool)> = HashMap::new();
    let device = enumerator.GetDefaultAudioEndpoint(eRender, eMultimedia)?;
    let session_manager: IAudioSessionManager2 =
        device.Activate(CLSCTX_ALL, None::<*const PROPVARIANT>)?;
    let session_enumerator = session_manager.GetSessionEnumerator()?;
    let count = session_enumerator.GetCount()?;
    for i in 0..count {
        if let Ok(session_control) = session_enumerator.GetSession(i) {
            let state = session_control.GetState()?;
            if state.0 == 2 {
                continue;
            }
            if let Ok(session_control2) = session_control.cast::<IAudioSessionControl2>() {
                let pid = session_control2.GetProcessId()?;
                if pid == 0 {
                    continue;
                }
                if let Ok(simple_volume) = session_control.cast::<ISimpleAudioVolume>() {
                    let vol = simple_volume.GetMasterVolume()?;
                    let mute = simple_volume.GetMute()?.as_bool();
                    let entry = session_map.entry(pid).or_insert((0.0, false));
                    if vol > entry.0 {
                        entry.0 = vol;
                        entry.1 = mute;
                    }
                }
            }
        }
    }
    let pids: Vec<u32> = session_map.keys().cloned().collect();
    update_cache_batch(&pids, cache);
    let mut apps = Vec::new();
    if let Ok(map) = cache.names.lock() {
        for (pid, (vol, mute)) in session_map {
            let (name, icon_path) = map
                .get(&pid)
                .cloned()
                .unwrap_or_else(|| (format!("App {}", pid), "".into()));
            apps.push(AppVolume {
                pid,
                name,
                volume: vol,
                is_muted: mute,
                icon_path,
            });
        }
    }
    apps.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(apps)
}
unsafe fn internal_set_app_vol(
    enumerator: &IMMDeviceEnumerator,
    target_pid: u32,
    vol: f32,
) -> Result<()> {
    let device = enumerator.GetDefaultAudioEndpoint(eRender, eMultimedia)?;
    let session_manager: IAudioSessionManager2 =
        device.Activate(CLSCTX_ALL, None::<*const PROPVARIANT>)?;
    let session_enumerator = session_manager.GetSessionEnumerator()?;
    for i in 0..session_enumerator.GetCount()? {
        if let Ok(session_control) = session_enumerator.GetSession(i) {
            if let Ok(sc2) = session_control.cast::<IAudioSessionControl2>() {
                if sc2.GetProcessId()? == target_pid {
                    if let Ok(sv) = session_control.cast::<ISimpleAudioVolume>() {
                        let _ = sv.SetMasterVolume(vol, std::ptr::null());
                    }
                }
            }
        }
    }
    Ok(())
}

unsafe fn internal_set_app_mute(
    enumerator: &IMMDeviceEnumerator,
    target_pid: u32,
    mute: bool,
) -> Result<()> {
    let device = enumerator.GetDefaultAudioEndpoint(eRender, eMultimedia)?;
    let session_manager: IAudioSessionManager2 =
        device.Activate(CLSCTX_ALL, None::<*const PROPVARIANT>)?;
    let session_enumerator = session_manager.GetSessionEnumerator()?;
    for i in 0..session_enumerator.GetCount()? {
        if let Ok(session_control) = session_enumerator.GetSession(i) {
            if let Ok(sc2) = session_control.cast::<IAudioSessionControl2>() {
                if sc2.GetProcessId()? == target_pid {
                    if let Ok(sv) = session_control.cast::<ISimpleAudioVolume>() {
                        let _ = sv.SetMute(mute, std::ptr::null());
                    }
                }
            }
        }
    }
    Ok(())
}

// PowerShell Script with C# embedding for safe Process Isolation
// This prevents STATUS_ACCESS_VIOLATION in the main app by moving the dangerous COM interaction to a child process.
const POWERSHELL_SWITCH_SCRIPT: &str = r#"
param($DeviceId)

$Source = @"
using System;
using System.Runtime.InteropServices;

[ComImport, Guid("870af99c-171d-4f9e-af0d-e63df40c2bc9")]
public class PolicyConfigClient { }

[ComImport, Guid("f8679f50-850a-41cf-9c72-430f290290c8"), InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
public interface IPolicyConfig {
    [PreserveSig] int GetMixFormat(string PCWSTR_id, out IntPtr __WAVEFORMATEX_ppFormat);
    [PreserveSig] int GetDeviceFormat(string PCWSTR_id, int INT_bDefault, out IntPtr __WAVEFORMATEX_ppFormat);
    [PreserveSig] int ResetDeviceFormat(string PCWSTR_id);
    [PreserveSig] int SetDeviceFormat(string PCWSTR_id, IntPtr __WAVEFORMATEX_pEndpointFormat, IntPtr __WAVEFORMATEX_pMixFormat);
    [PreserveSig] int GetProcessingPeriod(string PCWSTR_id, int INT_bDefault, out long __PINT64_pmftDefaultPeriod, out long __PINT64_pmftMinimumPeriod);
    [PreserveSig] int SetProcessingPeriod(string PCWSTR_id, long __PINT64_pmftPeriod);
    [PreserveSig] int GetShareMode(string PCWSTR_id, out IntPtr __DeviceShareMode_pMode);
    [PreserveSig] int SetShareMode(string PCWSTR_id, IntPtr __DeviceShareMode_mode);
    [PreserveSig] int GetPropertyValue(string PCWSTR_id, IntPtr __PROPERTYKEY_pKey, out IntPtr __PROPVARIANT_pv);
    [PreserveSig] int SetPropertyValue(string PCWSTR_id, IntPtr __PROPERTYKEY_pKey, IntPtr __PROPVARIANT_pv);
    [PreserveSig] int SetDefaultEndpoint(string PCWSTR_id, int INT_role);
    [PreserveSig] int SetEndpointVisibility(string PCWSTR_id, int INT_bVisible);
}

public class AudioSwitcher {
    public static int SetDefault(string deviceId) {
        try {
            var policyConfig = new PolicyConfigClient();
            var policyInterface = (IPolicyConfig)policyConfig;
            // Role: 0=eConsole, 1=eMultimedia, 2=eCommunications
            policyInterface.SetDefaultEndpoint(deviceId, 0); 
            policyInterface.SetDefaultEndpoint(deviceId, 1);
            policyInterface.SetDefaultEndpoint(deviceId, 2);
            return 0;
        } catch (Exception e) {
            Console.WriteLine("Error: " + e.Message);
            return 1;
        }
    }
}
"@

Add-Type -TypeDefinition $Source -Language CSharp
[AudioSwitcher]::SetDefault($DeviceId)
"#;

unsafe fn set_default_device(id: &str) -> Result<()> {
    println!(
        "DEBUG: AUDIO FIX V30 (Process Isolation) START for device {}",
        id
    );

    use std::io::Write;
    use std::process::Command;

    // 1. Prepare temp script path
    let mut temp_path = std::env::temp_dir();
    temp_path.push("tauri_audio_switch_v30.ps1");

    // 2. Write script to file
    if let Ok(mut file) = std::fs::File::create(&temp_path) {
        if let Err(e) = file.write_all(POWERSHELL_SWITCH_SCRIPT.as_bytes()) {
            println!("ERROR: Failed to write temp PowerShell script: {}", e);
            return Ok(());
        }
    } else {
        println!("ERROR: Failed to create temp file for PowerShell script");
        return Ok(());
    }

    // 3. Execute PowerShell
    println!("  Executing PowerShell wrapper...");
    use std::os::windows::process::CommandExt;
    let output = Command::new("powershell")
        .creation_flags(0x08000000) // CREATE_NO_WINDOW
        .args(&[
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            temp_path.to_str().unwrap_or(""),
            "-DeviceId",
            id,
        ])
        .output();

    // 4. Handle Result
    match output {
        Ok(out) => {
            if out.status.success() {
                println!("  Process exited successfully.");
                // Optional: verify
                thread::sleep(std::time::Duration::from_millis(150));
                if verify_default_device(id) {
                    println!("  SUCCESS: Device switched verified.");
                } else {
                    println!("  WARNING: Process finished but verification failed.");
                }
            } else {
                println!("  Process exited with error code: {:?}", out.status.code());
                let stderr = String::from_utf8_lossy(&out.stderr);
                println!("  STDERR: {}", stderr);
            }
        }
        Err(e) => {
            println!("  ERROR: Failed to spawn PowerShell process: {}", e);
        }
    }

    Ok(())
}

unsafe fn verify_default_device(target_id: &str) -> bool {
    if let Ok(enumerator) =
        CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL)
    {
        for role in [eMultimedia, eConsole, eCommunications] {
            if let Ok(def_dev) = enumerator.GetDefaultAudioEndpoint(eRender, role) {
                if let Ok(def_id) = def_dev.GetId() {
                    let s = def_id.to_string().unwrap_or_default();
                    println!("    Verify Role {:?}: {}", role, s);
                    CoTaskMemFree(Some(def_id.as_ptr() as *const c_void));
                    if s.to_lowercase() == target_id.to_lowercase() {
                        if role == eMultimedia {
                            return true;
                        }
                    }
                }
            }
        }
    }
    false
}

pub unsafe fn get_audio_endpoints(
    enumerator: &IMMDeviceEnumerator,
    data_flow: EDataFlow,
) -> Result<Vec<AudioDevice>> {
    let mut devices = Vec::new();
    let collection = enumerator.EnumAudioEndpoints(data_flow, DEVICE_STATE_ACTIVE)?;
    let count = collection.GetCount()?;
    for i in 0..count {
        let device = collection.Item(i)?;
        if let Ok(id_ptr) = device.GetId() {
            let id = id_ptr.to_string().unwrap_or_default();
            CoTaskMemFree(Some(id_ptr.as_ptr() as *const c_void));
            let props = device.OpenPropertyStore(STGM_READ)?;
            let mut name = String::new();
            if let Ok(mut val) = props.GetValue(&PKEY_Device_FriendlyName) {
                if !val.Anonymous.Anonymous.Anonymous.pwszVal.is_null() {
                    name = val
                        .Anonymous
                        .Anonymous
                        .Anonymous
                        .pwszVal
                        .to_string()
                        .unwrap_or_default();
                }
                let _ = PropVariantClear(&mut val as *mut _);
            }
            devices.push(AudioDevice {
                id: id.clone(),
                name: if name.is_empty() {
                    id.clone()
                } else {
                    name.clone()
                },
                is_default: false,
            });
        }
    }
    if let Ok(def_dev) = enumerator.GetDefaultAudioEndpoint(data_flow, eMultimedia) {
        if let Ok(def_id) = def_dev.GetId() {
            let s = def_id.to_string().unwrap_or_default();
            for d in &mut devices {
                if d.id == s {
                    d.is_default = true;
                }
            }
            CoTaskMemFree(Some(def_id.as_ptr() as *const c_void));
        }
    }
    devices.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(devices)
}

fn update_cache_batch(pids: &[u32], cache: &AppCache) {
    let mut missing_pids = Vec::new();
    if let Ok(map) = cache.names.lock() {
        for &pid in pids {
            if !map.contains_key(&pid) {
                missing_pids.push(pid);
            }
        }
    }
    if missing_pids.is_empty() {
        return;
    }
    let mut found_names = HashMap::new();
    unsafe {
        if let Ok(handle) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) {
            let mut pe = PROCESSENTRY32W {
                dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
                ..Default::default()
            };
            if Process32FirstW(handle, &mut pe).is_ok() {
                loop {
                    if missing_pids.contains(&pe.th32ProcessID) {
                        let name = String::from_utf16_lossy(&pe.szExeFile)
                            .trim_matches('\0')
                            .to_string();
                        let mut path = String::new();
                        if let Ok(h_proc) = OpenProcess(
                            PROCESS_QUERY_LIMITED_INFORMATION,
                            windows::Win32::Foundation::FALSE,
                            pe.th32ProcessID,
                        ) {
                            let mut buffer = [0u16; 1024];
                            let mut size = buffer.len() as u32;
                            if QueryFullProcessImageNameW(
                                h_proc,
                                PROCESS_NAME_WIN32,
                                PWSTR(buffer.as_mut_ptr()),
                                &mut size,
                            )
                            .is_ok()
                            {
                                path = String::from_utf16_lossy(&buffer[..size as usize]);
                            }
                            let _ = CloseHandle(h_proc);
                        }
                        let icon_b64 = if !path.is_empty() {
                            get_icon_as_base64(&path)
                        } else {
                            String::new()
                        };
                        found_names.insert(pe.th32ProcessID, (name, icon_b64));
                    }
                    if Process32NextW(handle, &mut pe).is_err() {
                        break;
                    }
                }
            }
            let _ = CloseHandle(handle);
        }
    }
    for &pid in &missing_pids {
        if !found_names.contains_key(&pid) {
            unsafe {
                if let Ok(handle) = OpenProcess(
                    PROCESS_QUERY_INFORMATION | PROCESS_VM_READ,
                    windows::Win32::Foundation::FALSE,
                    pid,
                ) {
                    let mut buf = [0u16; 1024];
                    let len = GetModuleBaseNameW(handle, None, &mut buf);
                    if len > 0 {
                        let name = String::from_utf16_lossy(&buf[..len as usize]);
                        let mut path = String::new();
                        if let Ok(h_proc) = OpenProcess(
                            PROCESS_QUERY_LIMITED_INFORMATION,
                            windows::Win32::Foundation::FALSE,
                            pid,
                        ) {
                            let mut buffer = [0u16; 1024];
                            let mut size = buffer.len() as u32;
                            if QueryFullProcessImageNameW(
                                h_proc,
                                PROCESS_NAME_WIN32,
                                PWSTR(buffer.as_mut_ptr()),
                                &mut size,
                            )
                            .is_ok()
                            {
                                path = String::from_utf16_lossy(&buffer[..size as usize]);
                            }
                            let _ = CloseHandle(h_proc);
                        }
                        let icon_b64 = if !path.is_empty() {
                            get_icon_as_base64(&path)
                        } else {
                            String::new()
                        };
                        found_names.insert(pid, (name, icon_b64));
                    }
                    let _ = CloseHandle(handle);
                }
            }
        }
    }
    if let Ok(mut map) = cache.names.lock() {
        for pid in missing_pids {
            let data = found_names
                .remove(&pid)
                .unwrap_or_else(|| (format!("App {}", pid), "".into()));
            map.insert(pid, data);
        }
    }
}

fn get_icon_as_base64(path: &str) -> String {
    unsafe {
        let path_v16: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
        let mut h_icons = [HICON::default(); 1];
        let mut path_fixed = [0u16; 260];
        let copy_len = path_v16.len().min(260);
        path_fixed[..copy_len].copy_from_slice(&path_v16[..copy_len]);
        let count = PrivateExtractIconsW(&path_fixed, 0, 48, 48, Some(&mut h_icons), None, 0);
        if count == 0 || h_icons[0].0 == 0 {
            let mut h_large = [HICON::default(); 1];
            if ExtractIconExW(
                windows::core::PCWSTR(path_v16.as_ptr()),
                0,
                Some(h_large.as_mut_ptr()),
                None,
                1,
            ) > 0
            {
                h_icons[0] = h_large[0];
            }
        }
        if h_icons[0].0 != 0 {
            let h_icon = h_icons[0];
            let mut icon_info = ICONINFO::default();
            if GetIconInfo(h_icon, &mut icon_info).is_ok() {
                let h_bm = if icon_info.hbmColor.0 != 0 {
                    icon_info.hbmColor
                } else {
                    icon_info.hbmMask
                };
                let mut bm = BITMAP::default();
                if GetObjectW(
                    h_bm,
                    std::mem::size_of::<BITMAP>() as i32,
                    Some(&mut bm as *mut _ as *mut _),
                ) > 0
                {
                    let width = bm.bmWidth;
                    let height = bm.bmHeight;
                    let hdc_screen = windows::Win32::Graphics::Gdi::GetDC(None);
                    let hdc_mem = CreateCompatibleDC(hdc_screen);
                    let old_bm = SelectObject(hdc_mem, h_bm);
                    let mut bmi = BITMAPINFO {
                        bmiHeader: BITMAPINFOHEADER {
                            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                            biWidth: width,
                            biHeight: -height,
                            biPlanes: 1,
                            biBitCount: 32,
                            biCompression: 0,
                            ..Default::default()
                        },
                        ..Default::default()
                    };
                    let mut buffer: Vec<u8> = vec![0; (width * height * 4) as usize];
                    let ret = GetDIBits(
                        hdc_mem,
                        h_bm,
                        0,
                        height as u32,
                        Some(buffer.as_mut_ptr() as *mut _),
                        &mut bmi,
                        DIB_RGB_COLORS,
                    );
                    if !old_bm.is_invalid() {
                        SelectObject(hdc_mem, old_bm);
                    }
                    let _ = DeleteDC(hdc_mem);
                    let _ = windows::Win32::Graphics::Gdi::ReleaseDC(None, hdc_screen);
                    if ret > 0 {
                        for chunk in buffer.chunks_exact_mut(4) {
                            chunk.swap(0, 2);
                        }
                        let mut png_data = Vec::new();
                        use image::ImageEncoder;
                        let encoder = image::codecs::png::PngEncoder::new(&mut png_data);
                        if encoder
                            .write_image(
                                &buffer,
                                width as u32,
                                height as u32,
                                image::ColorType::Rgba8.into(),
                            )
                            .is_ok()
                        {
                            let b64 = general_purpose::STANDARD.encode(png_data);
                            if icon_info.hbmColor.0 != 0 {
                                let _ = DeleteObject(icon_info.hbmColor);
                            }
                            if icon_info.hbmMask.0 != 0 {
                                let _ = DeleteObject(icon_info.hbmMask);
                            }
                            let _ = DestroyIcon(h_icon);
                            return format!("data:image/png;base64,{}", b64);
                        }
                    }
                }
                if icon_info.hbmColor.0 != 0 {
                    let _ = DeleteObject(icon_info.hbmColor);
                }
                if icon_info.hbmMask.0 != 0 {
                    let _ = DeleteObject(icon_info.hbmMask);
                }
            }
            let _ = DestroyIcon(h_icon);
        }
    }
    String::new()
}
