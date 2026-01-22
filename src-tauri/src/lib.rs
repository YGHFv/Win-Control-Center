mod audio;
mod display;
mod input;

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{
    menu::{CheckMenuItem, Menu, MenuItem, Submenu},
    tray::{MouseButton, TrayIconBuilder, TrayIconEvent},
    Manager,
};
use winreg::{enums::*, RegKey};

#[derive(Clone, PartialEq)]
struct LastTrayState {
    out_devs: Vec<audio::AudioDevice>,
    in_devs: Vec<audio::AudioDevice>,
    autostart: bool,
}

pub struct AppState {
    pub is_visible: AtomicBool,
    pub last_blur: AtomicU64,
    pub last_show: AtomicU64,
    last_tray_state: Mutex<Option<LastTrayState>>,
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

// --- Getter Commands (Using Request/Response) ---

#[tauri::command]
async fn get_system_volume(state: tauri::State<'_, audio::AudioState>) -> Result<f32, String> {
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
async fn get_mic_volume(state: tauri::State<'_, audio::AudioState>) -> Result<f32, String> {
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
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.set_size(tauri::Size::Logical(tauri::LogicalSize {
            width: 360.0,
            height,
        }));
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
                last_tray_state: Mutex::new(None),
            });

            // Initial tray menu update
            update_tray_menu(app.handle());

            // Background tray menu updater loop
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    update_tray_menu(&handle);
                }
            });

            // No initial menu needed, we build on click
            let _tray = TrayIconBuilder::with_id("main")
                .icon(app.default_window_icon().unwrap().clone())
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| {
                    let id_str = event.id().as_ref();
                    if id_str == "quit" {
                        app.exit(0);
                    } else if id_str == "autostart" {
                        let current = get_autostart();
                        let _ = set_autostart(!current);
                    } else if let Some(dev_id) = id_str.strip_prefix("out:") {
                        let state = app.state::<audio::AudioState>();
                        let _ = state
                            .tx
                            .send(audio::AudioRequest::SetDefaultDevice(dev_id.to_string()));
                    } else if let Some(dev_id) = id_str.strip_prefix("in:") {
                        let state = app.state::<audio::AudioState>();
                        let _ = state
                            .tx
                            .send(audio::AudioRequest::SetDefaultDevice(dev_id.to_string()));
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

                                    let win_size =
                                        window.outer_size().unwrap_or(tauri::PhysicalSize {
                                            width: 360,
                                            height: 400,
                                        });
                                    let (tx, ty) = match rect.position {
                                        tauri::Position::Physical(p) => (p.x, p.y),
                                        tauri::Position::Logical(l) => (l.x as i32, l.y as i32),
                                    };
                                    let tw = match rect.size {
                                        tauri::Size::Physical(s) => s.width,
                                        tauri::Size::Logical(l) => l.width as u32,
                                    };
                                    let x = tx + (tw as i32 / 2) - (win_size.width as i32 / 2);
                                    let mut y = ty - win_size.height as i32 - 10;
                                    if y < 0 {
                                        y = ty + 40;
                                    }

                                    let _ = window.set_position(tauri::Position::Physical(
                                        tauri::PhysicalPosition { x, y },
                                    ));
                                    let _ = window.show();
                                    let _ = window.set_focus();
                                }
                            }
                        }
                        _ => {}
                    }
                })
                .build(app)?;

            if let Some(window) = app.get_webview_window("main") {
                let w = window.clone();
                let app_handle = app.handle().clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::Focused(false) = event {
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
                });
            }

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_system_volume,
            set_system_volume,
            get_mic_volume,
            set_mic_volume,
            get_app_volumes,
            set_app_volume,
            get_brightness,
            set_brightness,
            get_mouse_speed,
            set_mouse_speed,
            resize_window
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn update_tray_menu(app_handle: &tauri::AppHandle) {
    let (out_devs, in_devs) = tauri::async_runtime::block_on(async {
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

        (out_devs, in_devs)
    });

    let is_auto = get_autostart();

    let new_state = LastTrayState {
        out_devs: out_devs.clone(),
        in_devs: in_devs.clone(),
        autostart: is_auto,
    };

    let app_state = app_handle.state::<AppState>();
    {
        let mut last = app_state.last_tray_state.lock().unwrap();
        if let Some(old) = &*last {
            if old == &new_state {
                return; // No change
            }
        }
        *last = Some(new_state);
    }

    let out_menu = Submenu::new(app_handle, "Sound Output", true).unwrap();
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

    let in_menu = Submenu::new(app_handle, "Sound Input", true).unwrap();
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

    let auto_item = CheckMenuItem::with_id(
        app_handle,
        "autostart",
        "Start on Boot",
        true,
        is_auto,
        None::<&str>,
    )
    .unwrap();

    let quit_item = MenuItem::with_id(app_handle, "quit", "Exit", true, None::<&str>).unwrap();

    let menu =
        Menu::with_items(app_handle, &[&out_menu, &in_menu, &auto_item, &quit_item]).unwrap();

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
