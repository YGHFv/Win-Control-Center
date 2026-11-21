mod audio;
mod display;
mod input;

#[cfg(target_os = "windows")]
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::window::Color;
use tauri::{
    image::Image,
    menu::{CheckMenuItem, Menu, MenuItem, Submenu},
    tray::{MouseButton, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager, Theme, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
};
#[cfg(target_os = "windows")]
use window_vibrancy::{apply_acrylic, apply_blur, apply_mica};
#[cfg(target_os = "windows")]
use windows::Win32::Foundation::{COLORREF, HWND};
#[cfg(target_os = "windows")]
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMSBT_TABBEDWINDOW, DWMWA_SYSTEMBACKDROP_TYPE,
};
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowLongW, SetLayeredWindowAttributes, SetWindowLongW, GWL_EXSTYLE, LWA_ALPHA,
    WS_EX_LAYERED,
};
use winreg::{enums::*, RegKey};

#[cfg(not(target_os = "windows"))]
fn apply_window_effect(_window: &WebviewWindow) {}

#[cfg(not(target_os = "windows"))]
fn set_window_alpha(_window: &WebviewWindow, _alpha: u8) -> Result<(), String> {
    Ok(())
}

// Embed icons at compile time for true portability
const ICON_WHITE_BYTES: &[u8] = include_bytes!("../icons/icon_white.png");
const ICON_BLACK_BYTES: &[u8] = include_bytes!("../icons/icon_black.png");
const TRAY_MENU_WIDTH: f64 = 192.0;
const TRAY_MENU_COMPACT_EXPANDED_WIDTH: f64 = 390.0;
const TRAY_MENU_DEVICE_EXPANDED_WIDTH: f64 = 460.0;
const TRAY_MENU_HEIGHT: f64 = 286.0;
const TRAY_MENU_EXPANDED_HEIGHT: f64 = 340.0;

fn tray_menu_expanded_width(submenu: Option<&str>) -> f64 {
    match submenu {
        Some("playback") | Some("recording") => TRAY_MENU_DEVICE_EXPANDED_WIDTH,
        Some(_) => TRAY_MENU_COMPACT_EXPANDED_WIDTH,
        None => TRAY_MENU_WIDTH,
    }
}

fn is_light_mode_registry() -> bool {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if let Ok(key) =
        hkcu.open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize")
    {
        if let Ok(val) = key.get_value::<u32, _>("SystemUsesLightTheme") {
            return val == 1;
        }
        if let Ok(val) = key.get_value::<u32, _>("AppsUseLightTheme") {
            return val == 1;
        }
    }
    false
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum BlurStyle {
    Mica,
    MicaAlt,
    Acrylic,
    Blur,
}

#[derive(Clone, PartialEq)]
struct LastTrayState {
    out_devs: Vec<audio::AudioDevice>,
    in_devs: Vec<audio::AudioDevice>,
    autostart: bool,
    blur_style: BlurStyle,
}

pub struct AppState {
    pub is_visible: AtomicBool,
    pub last_blur: AtomicU64,
    pub last_show: AtomicU64,
    pub height_cache: Mutex<f64>,
    pub blur_style: Mutex<BlurStyle>,

    last_tray_state: Mutex<Option<LastTrayState>>,
    tray: Mutex<Option<tauri::tray::TrayIcon>>,
}

#[derive(serde::Serialize)]
struct TrayMenuDevice {
    id: String,
    name: String,
    is_default: bool,
}

#[derive(serde::Serialize)]
struct TrayMenuState {
    playback_devices: Vec<TrayMenuDevice>,
    recording_devices: Vec<TrayMenuDevice>,
    autostart: bool,
    blur_style: String,
    controls: PanelVisibility,
}

#[derive(Clone, Copy, PartialEq, serde::Serialize)]
struct PanelVisibility {
    speaker: bool,
    microphone: bool,
    brightness: bool,
    mouse_speed: bool,
    volume_mixer: bool,
}

fn blur_style_key(style: BlurStyle) -> &'static str {
    match style {
        BlurStyle::Mica => "mica",
        BlurStyle::MicaAlt => "mica_alt",
        BlurStyle::Acrylic => "acrylic",
        BlurStyle::Blur => "blur",
    }
}

fn panel_visibility_value_name(key: &str) -> Option<&'static str> {
    match key {
        "speaker" => Some("ShowSpeaker"),
        "microphone" => Some("ShowMicrophone"),
        "brightness" => Some("ShowBrightness"),
        "mouse_speed" => Some("ShowMouseSpeed"),
        "volume_mixer" => Some("ShowVolumeMixer"),
        _ => None,
    }
}

fn read_panel_visibility() -> PanelVisibility {
    let defaults = PanelVisibility {
        speaker: true,
        microphone: true,
        brightness: true,
        mouse_speed: true,
        volume_mixer: true,
    };

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let Ok(key) = hkcu.open_subkey("Software\\WinControlCenter") else {
        return defaults;
    };

    let read_bool = |name: &str, default: bool| -> bool {
        key.get_value::<u32, _>(name)
            .map(|value| value != 0)
            .unwrap_or(default)
    };

    PanelVisibility {
        speaker: read_bool("ShowSpeaker", defaults.speaker),
        microphone: read_bool("ShowMicrophone", defaults.microphone),
        brightness: read_bool("ShowBrightness", defaults.brightness),
        mouse_speed: read_bool("ShowMouseSpeed", defaults.mouse_speed),
        volume_mixer: read_bool("ShowVolumeMixer", defaults.volume_mixer),
    }
}

fn panel_visibility_enabled(visibility: PanelVisibility, key: &str) -> Option<bool> {
    match key {
        "speaker" => Some(visibility.speaker),
        "microphone" => Some(visibility.microphone),
        "brightness" => Some(visibility.brightness),
        "mouse_speed" => Some(visibility.mouse_speed),
        "volume_mixer" => Some(visibility.volume_mixer),
        _ => None,
    }
}

fn set_panel_visibility_value(key: &str, enabled: bool) -> Result<(), String> {
    let value_name = panel_visibility_value_name(key)
        .ok_or_else(|| format!("Unknown panel visibility key: {}", key))?;
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (settings, _) = hkcu
        .create_subkey("Software\\WinControlCenter")
        .map_err(|e| e.to_string())?;
    settings
        .set_value(value_name, &(enabled as u32))
        .map_err(|e| e.to_string())
}

// --- Async Setter Commands (Non-blocking) ---

#[tauri::command]
fn set_system_volume(state: tauri::State<audio::AudioState>, vol: f32) {
    let _ = state.tx.send(audio::AudioRequest::SetMasterVolume(vol));
}

#[tauri::command]
fn set_mic_volume(state: tauri::State<audio::AudioState>, vol: f32) {
    let _ = state.tx.send(audio::AudioRequest::SetMicVolume(vol));
}

#[tauri::command]
fn set_app_volume(state: tauri::State<audio::AudioState>, pid: u32, vol: f32) {
    let _ = state.tx.send(audio::AudioRequest::SetAppVolume(pid, vol));
}

#[tauri::command]
fn set_app_mute(state: tauri::State<audio::AudioState>, pid: u32, mute: bool) {
    let _ = state.tx.send(audio::AudioRequest::SetAppMute(pid, mute));
}

#[tauri::command]
fn set_system_mute(state: tauri::State<audio::AudioState>, mute: bool) {
    let _ = state.tx.send(audio::AudioRequest::SetMasterMute(mute));
}

#[tauri::command]
fn set_mic_mute(state: tauri::State<audio::AudioState>, mute: bool) {
    let _ = state.tx.send(audio::AudioRequest::SetMicMute(mute));
}

// --- Getter Commands (Using Request/Response) ---

#[tauri::command]
async fn get_system_volume(
    state: tauri::State<'_, audio::AudioState>,
) -> Result<(f32, bool), String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    state
        .tx
        .send(audio::AudioRequest::GetMasterVolume(tx))
        .map_err(|e| e.to_string())?;
    rx.await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_mic_volume(state: tauri::State<'_, audio::AudioState>) -> Result<(f32, bool), String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    state
        .tx
        .send(audio::AudioRequest::GetMicVolume(tx))
        .map_err(|e| e.to_string())?;
    rx.await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_app_volumes(
    state: tauri::State<'_, audio::AudioState>,
) -> Result<Vec<audio::AppVolume>, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    state
        .tx
        .send(audio::AudioRequest::GetAppVolumes(tx))
        .map_err(|e| e.to_string())?;
    rx.await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn reapply_effects(window: tauri::WebviewWindow) {
    #[cfg(target_os = "windows")]
    {
        println!("Manually re-applying effects with DWM Kick...");
        let app_state = window.state::<AppState>();
        let should_restore_alpha = app_state.is_visible.load(Ordering::SeqCst);
        let _ = set_window_alpha(&window, 0);

        // 1. Force Resize (Kick DWM composition)
        let _ = window.set_size(tauri::Size::Logical(tauri::LogicalSize {
            width: 361.0,
            height: 400.0,
        }));
        std::thread::sleep(std::time::Duration::from_millis(10));
        let _ = window.set_size(tauri::Size::Logical(tauri::LogicalSize {
            width: 360.0,
            height: 400.0,
        }));

        // 2. Toggle Shadow (Reset border rendering)
        let _ = window.set_shadow(false);
        std::thread::sleep(std::time::Duration::from_millis(10));
        let _ = window.set_shadow(true);

        // 3. Apply Effect (Mica Alt Custom)
        // Note: apply_window_effect takes &WebviewWindow.
        apply_window_effect(&window);

        // 4. Clear Background (CRITICAL: Must happen after Mica Alt)
        let _ = window.set_background_color(Some(Color(0, 0, 0, 0)));
        if should_restore_alpha {
            std::thread::sleep(std::time::Duration::from_millis(35));
            let _ = set_window_alpha(&window, 255);
        }
    }
}

#[cfg(target_os = "windows")]
fn window_hwnd(window: &WebviewWindow) -> Result<HWND, String> {
    let handle = window.window_handle().map_err(|e| e.to_string())?;
    let raw = handle.as_raw();

    let hwnd_isize = match raw {
        RawWindowHandle::Win32(h) => h.hwnd.get(),
        _ => return Err("Not a Windows window".to_string()),
    };

    Ok(HWND(hwnd_isize))
}

#[cfg(target_os = "windows")]
fn set_window_alpha(window: &WebviewWindow, alpha: u8) -> Result<(), String> {
    let hwnd = window_hwnd(window)?;

    unsafe {
        let style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
        let layered_style = style | WS_EX_LAYERED.0;
        SetWindowLongW(hwnd, GWL_EXSTYLE, layered_style as i32);
        SetLayeredWindowAttributes(hwnd, COLORREF(0), alpha, LWA_ALPHA)
            .map_err(|e| e.to_string())?;
    }

    Ok(())
}

#[cfg(target_os = "windows")]
fn apply_window_effect(window: &WebviewWindow) {
    let is_light = is_light_mode_registry();
    let state = window.state::<AppState>();
    let style = *state.blur_style.lock().unwrap();

    println!(
        "Applying transparency effect (Light Mode: {}). Style: {:?}",
        is_light, style
    );

    let _ = window.set_background_color(Some(Color(0, 0, 0, 0)));

    // CRITICAL: Always reset DWM backdrop to NONE first to prevent stuck states
    let _ = reset_mica_custom(window);

    // KICK DWM: Toggle shadow off/on to force repaint of non-client area
    // This is often required when switching between Mica and Acrylic/Blur
    let _ = window.set_shadow(false);
    let _ = window.set_shadow(true);

    // Tiny sleep to ensure DWM catches up (prevent black flash artifact)
    std::thread::sleep(std::time::Duration::from_millis(20));

    let is_dark_mode = !is_light;
    let acrylic_tint = if is_light {
        (245, 245, 245, 72)
    } else {
        (28, 28, 28, 96)
    };
    let blur_tint = if is_light {
        (255, 255, 255, 32)
    } else {
        (24, 24, 24, 72)
    };

    let res = match style {
        BlurStyle::Mica => apply_mica(window, Some(is_dark_mode)).map_err(|e| format!("{:?}", e)),
        BlurStyle::MicaAlt => apply_mica_alt_custom(window),
        BlurStyle::Acrylic => {
            apply_acrylic(window, Some(acrylic_tint)).map_err(|e| format!("{:?}", e))
        }
        BlurStyle::Blur => {
            apply_blur(window, Some(blur_tint)).map_err(|e| format!("{:?}", e))
        }
    };

    if let Err(e) = res {
        println!("{:?} failed: {}. Fallback to Mica Alt...", style, e);
        if let Err(e2) = apply_mica_alt_custom(window) {
            println!("Fallback Mica Alt also failed: {:?}", e2);
        }
    } else {
        println!("{:?} applied successfully.", style);
    }

    // WebView2 transparency on Windows requires alpha 0. Non-zero alpha can be
    // treated as an opaque layer and makes Acrylic/Blur look like solid white.
    let _ = window.set_background_color(Some(Color(0, 0, 0, 0)));
}

#[cfg(target_os = "windows")]
#[cfg(target_os = "windows")]
fn reset_mica_custom(window: &WebviewWindow) -> Result<(), String> {
    let handle = window.window_handle().map_err(|e| e.to_string())?;
    let raw = handle.as_raw();

    let hwnd_isize = match raw {
        RawWindowHandle::Win32(h) => h.hwnd.get(),
        _ => return Err("Not a Windows window".to_string()),
    };

    let hwnd = HWND(hwnd_isize);

    unsafe {
        // Reset to DWMSBT_NONE before applying the next effect. AUTO can leave
        // stale black surfaces when switching between Mica, Acrylic and Blur.
        let val: u32 = 1;
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_SYSTEMBACKDROP_TYPE,
            &val as *const _ as *const _,
            std::mem::size_of::<u32>() as u32,
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn apply_mica_alt_custom(window: &WebviewWindow) -> Result<(), String> {
    let handle = window.window_handle().map_err(|e| e.to_string())?;
    let raw = handle.as_raw();

    let hwnd_isize = match raw {
        RawWindowHandle::Win32(h) => h.hwnd.get(),
        _ => return Err("Not a Windows window".to_string()),
    };

    let hwnd = HWND(hwnd_isize);

    unsafe {
        let val = DWMSBT_TABBEDWINDOW; // 4 = DWMSBT_TABBEDWINDOW
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_SYSTEMBACKDROP_TYPE,
            &val as *const _ as *const _,
            std::mem::size_of::<u32>() as u32,
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
async fn get_monitor_brightness(
    state: tauri::State<'_, display::BrightnessState>,
) -> Result<Vec<display::MonitorBrightness>, String> {
    display::get_monitor_brightness(&state).await
}

#[tauri::command]
async fn set_monitor_brightness(
    state: tauri::State<'_, display::BrightnessState>,
    id: String,
    val: f32,
) -> Result<(), String> {
    display::set_monitor_brightness(&state, id, val).await
}

#[tauri::command]
fn get_mouse_speed() -> u32 {
    input::get_mouse_sensitivity().unwrap_or(10)
}

#[tauri::command]
fn set_mouse_speed(val: u32) {
    let _ = input::set_mouse_sensitivity(val);
}

#[tauri::command]
fn get_panel_visibility() -> PanelVisibility {
    read_panel_visibility()
}

async fn get_audio_devices(
    app: &tauri::AppHandle,
) -> (Vec<audio::AudioDevice>, Vec<audio::AudioDevice>) {
    let audio_state = app.state::<audio::AudioState>();

    let (tx, rx) = tokio::sync::oneshot::channel();
    let _ = audio_state
        .tx
        .send(audio::AudioRequest::GetPlaybackDevices(tx));
    let playback = rx.await.ok().and_then(|r| r.ok()).unwrap_or_default();

    let (tx, rx) = tokio::sync::oneshot::channel();
    let _ = audio_state
        .tx
        .send(audio::AudioRequest::GetCaptureDevices(tx));
    let recording = rx.await.ok().and_then(|r| r.ok()).unwrap_or_default();

    (playback, recording)
}

#[tauri::command]
async fn get_tray_menu_state(app: tauri::AppHandle) -> Result<TrayMenuState, String> {
    let (playback, recording) = get_audio_devices(&app).await;
    let app_state = app.state::<AppState>();
    let current_style = *app_state.blur_style.lock().unwrap();

    Ok(TrayMenuState {
        playback_devices: playback
            .into_iter()
            .map(|d| TrayMenuDevice {
                id: d.id,
                name: d.name,
                is_default: d.is_default,
            })
            .collect(),
        recording_devices: recording
            .into_iter()
            .map(|d| TrayMenuDevice {
                id: d.id,
                name: d.name,
                is_default: d.is_default,
            })
            .collect(),
        autostart: get_autostart(),
        blur_style: blur_style_key(current_style).to_string(),
        controls: read_panel_visibility(),
    })
}

#[tauri::command]
async fn tray_menu_action(
    app: tauri::AppHandle,
    action: String,
    value: Option<String>,
) -> Result<(), String> {
    match action.as_str() {
        "quit" => {
            app.exit(0);
        }
        "autostart" => {
            let current = get_autostart();
            set_autostart(!current)?;
        }
        "playback" => {
            if let Some(device_id) = value {
                let state = app.state::<audio::AudioState>();
                let _ = state
                    .tx
                    .send(audio::AudioRequest::SetDefaultDevice(device_id));
            }
        }
        "recording" => {
            if let Some(device_id) = value {
                let state = app.state::<audio::AudioState>();
                let _ = state
                    .tx
                    .send(audio::AudioRequest::SetDefaultDevice(device_id));
            }
        }
        "style" => {
            let style_key = value.unwrap_or_else(|| "mica_alt".to_string());
            let new_style = match style_key.as_str() {
                "mica" => BlurStyle::Mica,
                "mica_alt" => BlurStyle::MicaAlt,
                "acrylic" => BlurStyle::Acrylic,
                "blur" => BlurStyle::Blur,
                _ => BlurStyle::MicaAlt,
            };

            {
                let state = app.state::<AppState>();
                *state.blur_style.lock().unwrap() = new_style;
            }
            set_saved_blur_style(new_style);

            if let Some(window) = app.get_webview_window("main") {
                reapply_effects(window);
            }
        }
        "control" => {
            if let Some(key) = value {
                let current_visibility = read_panel_visibility();
                let current = panel_visibility_enabled(current_visibility, &key)
                    .ok_or_else(|| format!("Unknown panel visibility key: {}", key))?;
                set_panel_visibility_value(&key, !current)?;
                let next_visibility = read_panel_visibility();
                let _ = app.emit("panel-visibility-changed", next_visibility);
            }
        }
        _ => {}
    }

    if action != "control" {
        if let Some(window) = app.get_webview_window("tray-menu") {
            let _ = window.hide();
        }
    }

    Ok(())
}

#[tauri::command]
fn set_tray_menu_expanded(app: tauri::AppHandle, expanded: bool, submenu: Option<String>) {
    if let Some(window) = app.get_webview_window("tray-menu") {
        let scale_factor = window.scale_factor().unwrap_or(1.0);
        let Ok(pos) = window.outer_position() else {
            return;
        };
        let Ok(size) = window.outer_size() else {
            return;
        };

        let width = if expanded {
            tray_menu_expanded_width(submenu.as_deref())
        } else {
            TRAY_MENU_WIDTH
        };
        let height = if expanded {
            TRAY_MENU_EXPANDED_HEIGHT
        } else {
            TRAY_MENU_HEIGHT
        };
        let new_height = (height * scale_factor).round() as i32;
        let bottom = pos.y + size.height as i32;

        let _ = window.set_size(tauri::Size::Logical(tauri::LogicalSize {
            width,
            height,
        }));
        let _ = window.set_position(tauri::Position::Physical(
            tauri::PhysicalPosition {
                x: pos.x,
                y: bottom - new_height,
            },
        ));
    }
}

#[tauri::command]
fn hide_tray_menu(app: tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("tray-menu") {
        let _ = window.hide();
    }
}

#[tauri::command]
async fn resize_window(app: tauri::AppHandle, height: f64) {
    let state = app.state::<AppState>();
    let mut cache = state.height_cache.lock().unwrap();
    let old_cache = *cache;
    *cache = height;

    if let Some(window) = app.get_webview_window("main") {
        let is_visible = state.is_visible.load(Ordering::SeqCst);
        if is_visible {
            // Only reposition if change is significant (> 2px) to avoid micro-jitters
            if (height - old_cache).abs() > 2.0 {
                let old_size = window.outer_size().unwrap_or_default();
                let scale_factor = window.scale_factor().unwrap_or(1.0);
                let new_height_phys = (height * scale_factor) as i32;
                let pos = window.outer_position().unwrap_or_default();

                let _ = window.set_size(tauri::Size::Logical(tauri::LogicalSize {
                    width: 360.0,
                    height,
                }));

                let diff = new_height_phys - old_size.height as i32;
                let _ = window.set_position(tauri::Position::Physical(
                    tauri::PhysicalPosition {
                        x: pos.x,
                        y: pos.y - diff,
                    },
                ));
            } else {
                // Near-zero change, just ensure size is synced without heavy movement
                // let _ = window.set_size(tauri::Size::Logical(tauri::LogicalSize {
                //     width: 360.0,
                //     height,
                // }));
            }
        } else {
            // let _ = window.set_size(tauri::Size::Logical(tauri::LogicalSize {
            //     width: 360.0,
            //     height,
            // }));
        }
    }
}

fn update_tray_icon_for_theme(app: &tauri::AppHandle, theme: Theme) {
    let icon_bytes = match theme {
        Theme::Light => ICON_BLACK_BYTES,
        _ => ICON_WHITE_BYTES,
    };

    println!(
        "System Theme changed to: {:?}, loading from embedded bytes",
        theme
    );

    if let Ok(icon) = Image::from_bytes(icon_bytes) {
        if let Some(tray) = app.tray_by_id("main") {
            let _ = tray.set_icon(Some(icon));
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let app_cache = Arc::new(audio::AppCache::new());
            app.manage(audio::AudioState::new(app_cache.clone()));
            app.manage(display::BrightnessState::default());

            app.manage(AppState {
                is_visible: AtomicBool::new(false),
                last_blur: AtomicU64::new(0),
                last_show: AtomicU64::new(0),
                height_cache: Mutex::new(400.0),
                blur_style: Mutex::new(get_saved_blur_style()),
                last_tray_state: Mutex::new(None),
                tray: Mutex::new(None),
            });

            // Setup tray
            let window = app.get_webview_window("main").unwrap();

            let _ = window.set_decorations(false);
            let _ = window.set_shadow(true); // RESTORE SHADOW: Required for rounded corners
            let _ = window.set_title("");

            // CRITICAL: Explicitly clear background color to ensure transparency
            let _ = window.set_background_color(Some(Color(0, 0, 0, 0)));
            let _ = set_window_alpha(&window, 0);
            let _ = window.set_position(tauri::Position::Physical(
                tauri::PhysicalPosition {
                    x: -32000,
                    y: -32000,
                },
            ));
            let _ = window.show();

            #[cfg(target_os = "windows")]
            {
                let w = window.clone();
                tauri::async_runtime::spawn(async move {
                    // Keep the WebView warm offscreen so tray toggles never expose
                    // WebView2's default white first frame.

                    // Initial apply
                    apply_window_effect(&w);

                    // Delayed fix - reduced time for faster startup
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                    apply_window_effect(&w);
                    let _ = w.set_background_color(Some(Color(0, 0, 0, 0)));
                    println!("VIBRANCY APPLIED: Mica Alt Custom + Clean");
                });
            }

            let tray_menu_window = WebviewWindowBuilder::new(
                app,
                "tray-menu",
                WebviewUrl::App("/#menu".into()),
            )
            .title("")
            .inner_size(TRAY_MENU_WIDTH, TRAY_MENU_HEIGHT)
            .decorations(false)
            .transparent(true)
            .background_color(Color(0, 0, 0, 0))
            .resizable(false)
            .visible(false)
            .always_on_top(true)
            .shadow(false)
            .skip_taskbar(true)
            .build()?;
            let _ = tray_menu_window.set_background_color(Some(Color(0, 0, 0, 0)));

            // Initial theme from registry (more reliable than window.theme() at start)
            let theme = if is_light_mode_registry() {
                Theme::Light
            } else {
                Theme::Dark
            };
            let icon_bytes = match theme {
                Theme::Light => ICON_BLACK_BYTES,
                _ => ICON_WHITE_BYTES,
            };
            let initial_icon = Image::from_bytes(icon_bytes)
                .unwrap_or_else(|_| app.default_window_icon().unwrap().clone());

            let _tray = TrayIconBuilder::with_id("main")
                .icon(initial_icon)
                .tooltip("Win Control Center")
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| {
                    let id_str = event.id().as_ref();
                    println!("Tray menu click: {}", id_str);
                    if id_str == "quit" {
                        app.exit(0);
                    } else if id_str == "autostart" {
                        let current = get_autostart();
                        let _ = set_autostart(!current);
                    } else if let Some(dev_id) = id_str.strip_prefix("out:") {
                        println!("Switching Playback to: {}", dev_id);
                        let state = app.state::<audio::AudioState>();
                        let _ = state
                            .tx
                            .send(audio::AudioRequest::SetDefaultDevice(dev_id.to_string()));

                        // Trigger immediate update
                        let h = app.clone();
                        tauri::async_runtime::spawn(async move {
                            tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
                            update_tray_menu(&h).await;
                        });
                    } else if let Some(style_str) = id_str.strip_prefix("style:") {
                        let new_style = match style_str {
                            "mica" => BlurStyle::Mica,
                            "mica_alt" => BlurStyle::MicaAlt,
                            "acrylic" => BlurStyle::Acrylic,
                            _ => BlurStyle::Blur,
                        };
                        {
                            let state = app.state::<AppState>();
                            *state.blur_style.lock().unwrap() = new_style;
                        }
                        set_saved_blur_style(new_style);
                        println!("Switched Blur Style to: {:?}", new_style);

                        // Re-apply immediate
                        if let Some(window) = app.get_webview_window("main") {
                            reapply_effects(window);
                        }

                        // Trigger menu update check
                        let h = app.clone();
                        tauri::async_runtime::spawn(async move {
                            update_tray_menu(&h).await;
                        });
                    } else if let Some(dev_id) = id_str.strip_prefix("in:") {
                        println!("Switching Recording to: {}", dev_id);
                        let state = app.state::<audio::AudioState>();
                        let _ = state
                            .tx
                            .send(audio::AudioRequest::SetDefaultDevice(dev_id.to_string()));

                        // Trigger immediate update
                        let h = app.clone();
                        tauri::async_runtime::spawn(async move {
                            tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
                            update_tray_menu(&h).await;
                        });
                    }
                })
                .on_tray_icon_event(|tray, event| {
                    match event {
                        TrayIconEvent::Click {
                            button: MouseButton::Right,
                            rect,
                            ..
                        } => {
                            let app = tray.app_handle();
                            if let Some(menu_window) = app.get_webview_window("tray-menu") {
                                let scale_factor = menu_window.scale_factor().unwrap_or(1.0);
                                let (tx, ty) = match rect.position {
                                    tauri::Position::Physical(p) => (p.x, p.y),
                                    tauri::Position::Logical(l) => (
                                        (l.x * scale_factor) as i32,
                                        (l.y * scale_factor) as i32,
                                    ),
                                };
                                let tw = match rect.size {
                                    tauri::Size::Physical(s) => s.width,
                                    tauri::Size::Logical(l) => (l.width * scale_factor) as u32,
                                };

                                let menu_width = (TRAY_MENU_WIDTH * scale_factor) as i32;
                                let max_expanded_width =
                                    (TRAY_MENU_DEVICE_EXPANDED_WIDTH * scale_factor) as i32;
                                let height = (TRAY_MENU_HEIGHT * scale_factor) as i32;
                                let mut x = tx + (tw as i32 / 2) - menu_width + 24;
                                let y = ty - height - 10;

                                if let Ok(Some(monitor)) = menu_window.current_monitor() {
                                    let monitor_pos = monitor.position();
                                    let monitor_size = monitor.size();
                                    let min_x = monitor_pos.x + 8;
                                    let max_x = monitor_pos.x + monitor_size.width as i32
                                        - max_expanded_width
                                        - 8;
                                    x = x.clamp(min_x, max_x.max(min_x));
                                }

                                let _ = menu_window.set_size(tauri::Size::Logical(
                                    tauri::LogicalSize {
                                        width: TRAY_MENU_WIDTH,
                                        height: TRAY_MENU_HEIGHT,
                                    },
                                ));
                                let _ = menu_window.set_position(tauri::Position::Physical(
                                    tauri::PhysicalPosition { x, y },
                                ));
                                let _ = menu_window.set_background_color(Some(Color(0, 0, 0, 0)));
                                let _ = menu_window.show();
                                let _ = menu_window.set_focus();
                            }
                        }
                        TrayIconEvent::Click {
                            button: MouseButton::Left,
                            rect,
                            ..
                        }
                        | TrayIconEvent::DoubleClick {
                            button: MouseButton::Left,
                            rect,
                            ..
                        } => {
                            let app = tray.app_handle();
                            let state = app.state::<AppState>();
                            let now = SystemTime::now()
                                .duration_since(UNIX_EPOCH)
                                .unwrap()
                                .as_millis() as u64;

                            if let Some(window) = app.get_webview_window("main") {
                                let is_panel_visible = state.is_visible.load(Ordering::SeqCst);

                                if is_panel_visible {
                                    // Protect against double-click/spam-click hiding
                                    let last_show_time = state.last_show.load(Ordering::SeqCst);
                                    if now - last_show_time < 500 {
                                        return;
                                    }
                                    let _ = set_window_alpha(&window, 0);
                                    let _ = window.set_position(tauri::Position::Physical(
                                        tauri::PhysicalPosition {
                                            x: -32000,
                                            y: -32000,
                                        },
                                    ));
                                    state.is_visible.store(false, Ordering::SeqCst);
                                    state.last_blur.store(now, Ordering::SeqCst);
                                } else {
                                    let last_blur_time = state.last_blur.load(Ordering::SeqCst);
                                    if now - last_blur_time < 250 {
                                        return;
                                    }
                                    state.last_show.store(now, Ordering::SeqCst);
                                    let _ = set_window_alpha(&window, 0);

                                    // Use cached height for initial sizing and positioning
                                    let cached_height = *state.height_cache.lock().unwrap();
                                    let _ =
                                        window.set_size(tauri::Size::Logical(tauri::LogicalSize {
                                            width: 360.0,
                                            height: cached_height,
                                        }));

                                    let scale_factor = window.scale_factor().unwrap_or(1.0);
                                    let (tx, ty) = match rect.position {
                                        tauri::Position::Physical(p) => (p.x, p.y),
                                        tauri::Position::Logical(l) => (
                                            (l.x * scale_factor) as i32,
                                            (l.y * scale_factor) as i32,
                                        ),
                                    };
                                    let tw = match rect.size {
                                        tauri::Size::Physical(s) => s.width,
                                        tauri::Size::Logical(l) => (l.width * scale_factor) as u32,
                                    };

                                    // DPI-aware physical size calculation
                                    let cached_height = *state.height_cache.lock().unwrap();
                                    let target_width_phys = (360.0 * scale_factor) as i32;
                                    let target_height_phys = (cached_height * scale_factor) as i32;

                                    let x = tx + (tw as i32 / 2) - (target_width_phys / 2);
                                    let y = ty - target_height_phys - 10;

                                    let _ = window.set_position(tauri::Position::Physical(
                                        tauri::PhysicalPosition { x, y },
                                    ));

                                    let _ = window.set_background_color(Some(Color(0, 0, 0, 0)));
                                    state.is_visible.store(true, Ordering::SeqCst);
                                    let _ = window.set_focus();
                                    let _ = set_window_alpha(&window, 255);
                                }
                            }
                        }
                        _ => {}
                    }
                })
                .build(app)?;

            if let Some(state) = app.try_state::<AppState>() {
                *state.tray.lock().unwrap() = Some(_tray);
            }

            if let Some(window) = app.get_webview_window("main") {
                let w = window.clone();
                let app_handle = app.handle().clone();
                window.on_window_event(move |event| match event {
                    tauri::WindowEvent::Focused(false) => {
                        let state = app_handle.state::<AppState>();
                        if !state.is_visible.load(Ordering::SeqCst) {
                            return;
                        }
                        let now = SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .unwrap()
                            .as_millis() as u64;

                        let last_show = state.last_show.load(Ordering::SeqCst);
                        if now - last_show < 300 {
                            return;
                        }

                        state.last_blur.store(now, Ordering::SeqCst);
                        let _ = set_window_alpha(&w, 0);
                        let _ = w.set_position(tauri::Position::Physical(
                            tauri::PhysicalPosition {
                                x: -32000,
                                y: -32000,
                            },
                        ));
                        state.is_visible.store(false, Ordering::SeqCst);
                    }
                    tauri::WindowEvent::ThemeChanged(theme) => {
                        update_tray_icon_for_theme(&app_handle, *theme);
                        // Re-apply window effect to match new theme
                        let w_clone = w.clone();
                        tauri::async_runtime::spawn(async move {
                            // Small delay to ensure registry/system state propagates if needed
                            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                            apply_window_effect(&w_clone);
                        });
                    }
                    _ => {}
                });
            }

            if let Some(window) = app.get_webview_window("tray-menu") {
                let w = window.clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::Focused(false) = event {
                        let _ = w.hide();
                    }
                });
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_system_volume,
            set_system_volume,
            get_mic_volume,
            set_mic_volume,
            set_system_mute,
            set_mic_mute,
            get_app_volumes,
            set_app_volume,
            set_app_mute,
            get_monitor_brightness,
            set_monitor_brightness,
            get_mouse_speed,
            set_mouse_speed,
            get_panel_visibility,
            resize_window,
            get_tray_menu_state,
            tray_menu_action,
            set_tray_menu_expanded,
            hide_tray_menu
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

async fn update_tray_menu(app_handle: &tauri::AppHandle) {
    let (out_devs, in_devs) = get_audio_devices(app_handle).await;
    let is_auto = get_autostart();
    let app_state = app_handle.state::<AppState>();
    let current_style = *app_state.blur_style.lock().unwrap();

    let new_state = LastTrayState {
        out_devs: out_devs.clone(),
        in_devs: in_devs.clone(),
        autostart: is_auto,
        blur_style: current_style,
    };

    {
        let mut last = app_state.last_tray_state.lock().unwrap();
        if let Some(old) = &*last {
            if old == &new_state {
                return;
            }
        }
        *last = Some(new_state);
    }

    let out_menu = Submenu::new(app_handle, "\u{64ad}\u{653e}\u{8bbe}\u{5907}", true).unwrap();
    for d in out_devs {
        let _ = out_menu.append(
            &CheckMenuItem::with_id(
                app_handle,
                format!("out:{}", d.id),
                &d.name,
                true,
                d.is_default,
                None::<&str>,
            )
            .unwrap(),
        );
    }

    let in_menu = Submenu::new(app_handle, "\u{5f55}\u{97f3}\u{8bbe}\u{5907}", true).unwrap();
    for d in in_devs {
        let _ = in_menu.append(
            &CheckMenuItem::with_id(
                app_handle,
                format!("in:{}", d.id),
                &d.name,
                true,
                d.is_default,
                None::<&str>,
            )
            .unwrap(),
        );
    }

    let style_menu = Submenu::new(app_handle, "\u{6a21}\u{7cca}\u{6837}\u{5f0f}", true).unwrap();
    let style_items = [
        ("style:mica", "\u{4e91}\u{6bcd} (Mica)", BlurStyle::Mica),
        (
            "style:mica_alt",
            "\u{4e91}\u{6bcd} Alt (Mica Alt)",
            BlurStyle::MicaAlt,
        ),
        (
            "style:acrylic",
            "\u{4e9a}\u{514b}\u{529b} (Acrylic)",
            BlurStyle::Acrylic,
        ),
        ("style:blur", "\u{6a21}\u{7cca} (Blur)", BlurStyle::Blur),
    ];
    for (id, label, style) in style_items {
        let _ = style_menu.append(
            &CheckMenuItem::with_id(
                app_handle,
                id,
                label,
                true,
                current_style == style,
                None::<&str>,
            )
            .unwrap(),
        );
    }

    let auto_item = CheckMenuItem::with_id(
        app_handle,
        "autostart",
        "\u{5f00}\u{673a}\u{81ea}\u{542f}",
        true,
        is_auto,
        None::<&str>,
    )
    .unwrap();

    let quit_item =
        MenuItem::with_id(app_handle, "quit", "\u{9000}\u{51fa}", true, None::<&str>).unwrap();

    let menu = Menu::with_items(
        app_handle,
        &[&out_menu, &in_menu, &style_menu, &auto_item, &quit_item],
    )
    .unwrap();

    if let Some(tray) = app_handle.tray_by_id("main") {
        let _ = tray.set_menu(Some(menu));
    }
}

fn get_autostart() -> bool {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if let Ok(run) = hkcu.open_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Run") {
        let app_name = "WinControlCenter";
        // Check if value exists
        return run.get_value::<String, _>(app_name).is_ok();
    }
    false
}

fn set_autostart(enable: bool) -> Result<(), String> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let run = hkcu
        .open_subkey_with_flags(
            "Software\\Microsoft\\Windows\\CurrentVersion\\Run",
            KEY_WRITE,
        )
        .map_err(|e| e.to_string())?;

    let app_name = "WinControlCenter";
    if enable {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let path = exe.to_str().unwrap_or_default();
        let val = format!("\"{}\"", path);
        run.set_value(app_name, &val).map_err(|e| e.to_string())?;
    } else {
        let _ = run.delete_value(app_name);
    }
    Ok(())
}

fn get_saved_blur_style() -> BlurStyle {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if let Ok(key) = hkcu.open_subkey("Software\\WinControlCenter") {
        if let Ok(val) = key.get_value::<String, _>("BlurStyle") {
            return match val.as_str() {
                "mica" => BlurStyle::Mica,
                "mica_alt" => BlurStyle::MicaAlt,
                "acrylic" => BlurStyle::Acrylic,
                "blur" => BlurStyle::Blur,
                _ => BlurStyle::MicaAlt,
            };
        }
    }
    BlurStyle::MicaAlt
}

fn set_saved_blur_style(style: BlurStyle) {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = match hkcu.create_subkey("Software\\WinControlCenter") {
        Ok(v) => v,
        Err(_) => return,
    };
    let val = match style {
        BlurStyle::Mica => "mica",
        BlurStyle::MicaAlt => "mica_alt",
        BlurStyle::Acrylic => "acrylic",
        BlurStyle::Blur => "blur",
    };
    let _ = key.set_value("BlurStyle", &val.to_string());
}
