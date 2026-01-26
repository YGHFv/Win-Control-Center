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
    Manager, Theme, WebviewWindow,
};
#[cfg(target_os = "windows")]
use window_vibrancy::{apply_acrylic, apply_blur, apply_mica};
#[cfg(target_os = "windows")]
use windows::Win32::Foundation::HWND;
#[cfg(target_os = "windows")]
use windows::Win32::Graphics::Dwm::{
    DwmSetWindowAttribute, DWMSBT_TABBEDWINDOW, DWMWA_SYSTEMBACKDROP_TYPE,
};
use winreg::{enums::*, RegKey};

#[cfg(not(target_os = "windows"))]
fn apply_window_effect(_window: &WebviewWindow) {}

// Embed icons at compile time for true portability
const ICON_WHITE_BYTES: &[u8] = include_bytes!("../icons/icon_white.png");
const ICON_BLACK_BYTES: &[u8] = include_bytes!("../icons/icon_black.png");

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
    }
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

    // CRITICAL: Always reset DWM backdrop to NONE first to prevent stuck states
    let _ = reset_mica_custom(window);

    // KICK DWM: Toggle shadow off/on to force repaint of non-client area
    // This is often required when switching between Mica and Acrylic/Blur
    let _ = window.set_shadow(false);
    let _ = window.set_shadow(true);

    // Tiny sleep to ensure DWM catches up (prevent black flash artifact)
    std::thread::sleep(std::time::Duration::from_millis(20));

    let is_dark_mode = !is_light;
    // Use fully transparent fallback to avoid black background artifacts in Acrylic/Blur
    let fallback_color = if is_light {
        (255, 255, 255, 0)
    } else {
        (0, 0, 0, 0)
    };

    let res = match style {
        BlurStyle::Mica => apply_mica(window, Some(is_dark_mode)).map_err(|e| format!("{:?}", e)),
        BlurStyle::MicaAlt => apply_mica_alt_custom(window),
        BlurStyle::Acrylic => {
            apply_acrylic(window, Some(fallback_color)).map_err(|e| format!("{:?}", e))
        }
        BlurStyle::Blur => apply_blur(window, Some(fallback_color)).map_err(|e| format!("{:?}", e)),
    };

    if let Err(e) = res {
        println!("{:?} failed: {}. Fallback to Blur...", style, e);
        if let Err(e2) = apply_blur(window, Some(fallback_color)) {
            println!("Fallback Blur also failed: {:?}", e2);
        }
    } else {
        println!("{:?} applied successfully.", style);
    }

    // Always clear background color at the end
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
        // Reset to DWMSBT_AUTO (0) or DWMSBT_NONE (1).
        // 0 resets to system default behavior which is safest for clearing overrides.
        let val: u32 = 0;
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

// --- Brightness with Smart Cache & De-duplication ---

pub struct BrightnessCache {
    val: Mutex<f32>,
    last_fetch: AtomicU64,
    is_fetching: AtomicBool,
}

#[tauri::command]
async fn get_brightness(cache: tauri::State<'_, BrightnessCache>) -> Result<f32, String> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let last = cache.last_fetch.load(Ordering::Relaxed);

    // 5s cache
    if last != 0 && now - last < 5 {
        return Ok(*cache.val.lock().unwrap());
    }

    if cache.is_fetching.swap(true, Ordering::SeqCst) {
        return Ok(*cache.val.lock().unwrap());
    }

    let res = display::get_brightness().await;
    cache.is_fetching.store(false, Ordering::SeqCst);

    let val = res?;
    if let Ok(mut v) = cache.val.lock() {
        *v = val;
    }
    cache.last_fetch.store(now, Ordering::Relaxed);
    Ok(val)
}

#[tauri::command]
async fn set_brightness(cache: tauri::State<'_, BrightnessCache>, val: f32) -> Result<(), String> {
    display::set_brightness(val).await?;
    if let Ok(mut v) = cache.val.lock() {
        *v = val;
    }
    cache.last_fetch.store(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        Ordering::Relaxed,
    );
    Ok(())
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
async fn resize_window(app: tauri::AppHandle, height: f64) {
    let state = app.state::<AppState>();
    let mut cache = state.height_cache.lock().unwrap();
    let old_cache = *cache;
    *cache = height;

    if let Some(window) = app.get_webview_window("main") {
        let is_visible = window.is_visible().unwrap_or(false);
        if is_visible {
            // Only reposition if change is significant (> 2px) to avoid micro-jitters
            if (height - old_cache).abs() > 2.0 {
                let _old_size = window.outer_size().unwrap_or_default();
                let scale_factor = window.scale_factor().unwrap_or(1.0);
                let _new_height_phys = (height * scale_factor) as i32;
                let _pos = window.outer_position().unwrap_or_default();

                let _ = window.set_size(tauri::Size::Logical(tauri::LogicalSize {
                    width: 360.0,
                    height,
                }));

                // Adjust Y to keep bottom fixed
                // let diff = new_height_phys - old_size.height as i32;
                // let _ = window.set_position(tauri::Position::Physical(tauri::PhysicalPosition {
                //     x: pos.x,
                //     y: pos.y - diff,
                // }));
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
            app.manage(BrightnessCache {
                val: Mutex::new(0.5),
                last_fetch: AtomicU64::new(0),
                is_fetching: AtomicBool::new(false),
            });

            app.manage(AppState {
                is_visible: AtomicBool::new(false),
                last_blur: AtomicU64::new(0),
                last_show: AtomicU64::new(0),
                height_cache: Mutex::new(400.0),
                blur_style: Mutex::new(BlurStyle::MicaAlt), // Default to Mica Alt
                last_tray_state: Mutex::new(None),
                tray: Mutex::new(None),
            });

            // Background tray menu updater loop
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                    update_tray_menu(&handle).await;
                }
            });

            // Setup tray
            let window = app.get_webview_window("main").unwrap();

            let _ = window.set_decorations(false);
            let _ = window.set_shadow(true); // RESTORE SHADOW: Required for rounded corners
            let _ = window.set_title("");

            // CRITICAL: Explicitly clear background color to ensure transparency
            let _ = window.set_background_color(Some(Color(0, 0, 0, 0)));

            #[cfg(target_os = "windows")]
            {
                let w = window.clone();
                tauri::async_runtime::spawn(async move {
                    // Final Attempt: Direct Startup Mica Alt Custom
                    // Using Mica Alt + Pure Transparent Background matches the "Visible: True" config

                    // Initial apply
                    apply_window_effect(&w);

                    // Delayed fix - reduced time for faster startup
                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                    apply_window_effect(&w);
                    let _ = w.set_background_color(Some(Color(0, 0, 0, 0)));
                    println!("VIBRANCY APPLIED: Mica Alt Custom + Clean");
                });
            }

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
                            ..
                        } => {
                            // Menu is updated in background loop
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
                                let is_physically_visible = window.is_visible().unwrap_or(false);

                                if is_physically_visible {
                                    // Protect against double-click/spam-click hiding
                                    let last_show_time = state.last_show.load(Ordering::SeqCst);
                                    if now - last_show_time < 500 {
                                        return;
                                    }
                                    let _ = window.hide();
                                    state.last_blur.store(now, Ordering::SeqCst);
                                } else {
                                    let last_blur_time = state.last_blur.load(Ordering::SeqCst);
                                    if now - last_blur_time < 250 {
                                        return;
                                    }
                                    state.last_show.store(now, Ordering::SeqCst);

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

                                    // Re-apply effect BEFORE showing to prevent white flash
                                    apply_window_effect(&window);

                                    let _ = window.show();
                                    let _ = window.set_focus();
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
                        let now = SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .unwrap()
                            .as_millis() as u64;

                        let last_show = state.last_show.load(Ordering::SeqCst);
                        if now - last_show < 300 {
                            return;
                        }

                        state.last_blur.store(now, Ordering::SeqCst);
                        let _ = w.hide();
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

            // Initial tray menu update
            let h2 = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                update_tray_menu(&h2).await;
            });

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
            get_brightness,
            set_brightness,
            get_mouse_speed,
            set_mouse_speed,
            resize_window
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

async fn update_tray_menu(app_handle: &tauri::AppHandle) {
    let audio_state = app_handle.state::<audio::AudioState>();
    let (tx, rx) = tokio::sync::oneshot::channel();
    let _ = audio_state
        .tx
        .send(audio::AudioRequest::GetPlaybackDevices(tx));
    let out_devs = rx.await.ok().and_then(|r| r.ok()).unwrap_or_default();

    let (tx, rx) = tokio::sync::oneshot::channel();
    let _ = audio_state
        .tx
        .send(audio::AudioRequest::GetCaptureDevices(tx));
    let in_devs = rx.await.ok().and_then(|r| r.ok()).unwrap_or_default();

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
                // Return if nothing changed to avoid closing the open context menu
                return;
            }
        }
        *last = Some(new_state);
    }

    let out_menu = Submenu::new(app_handle, "播放设备", true).unwrap();
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

    let in_menu = Submenu::new(app_handle, "录音设备", true).unwrap();
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

    let style_menu = Submenu::new(app_handle, "模糊样式", true).unwrap();
    let _ = style_menu.append(
        &CheckMenuItem::with_id(
            app_handle,
            "style:mica",
            "云母 (Mica)",
            true,
            current_style == BlurStyle::Mica,
            None::<&str>,
        )
        .unwrap(),
    );
    let _ = style_menu.append(
        &CheckMenuItem::with_id(
            app_handle,
            "style:mica_alt",
            "云母 Alt (Mica Alt)",
            true,
            current_style == BlurStyle::MicaAlt,
            None::<&str>,
        )
        .unwrap(),
    );
    let _ = style_menu.append(
        &CheckMenuItem::with_id(
            app_handle,
            "style:acrylic",
            "亚克力 (Acrylic)",
            true,
            current_style == BlurStyle::Acrylic,
            None::<&str>,
        )
        .unwrap(),
    );

    let auto_item = CheckMenuItem::with_id(
        app_handle,
        "autostart",
        "开机自启",
        true,
        is_auto,
        None::<&str>,
    )
    .unwrap();

    let quit_item = MenuItem::with_id(app_handle, "quit", "退出", true, None::<&str>).unwrap();

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
