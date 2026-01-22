use brightness::Brightness;
use futures::stream::TryStreamExt;
use std::os::windows::process::CommandExt;
use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const CREATE_NO_WINDOW: u32 = 0x08000000;

// Global cache to prevent PowerShell spamming and main thread blocking
static BRIGHTNESS_CACHE: Mutex<(f32, Option<Instant>)> = Mutex::new((0.5, None));

pub async fn get_brightness() -> Result<f32, String> {
    {
        let cache = BRIGHTNESS_CACHE.lock().unwrap();
        if let Some(last_time) = cache.1 {
            if last_time.elapsed() < Duration::from_millis(5000) {
                return Ok(cache.0);
            }
        }
    }

    println!("DEBUG: Fetching brightness devices via Crate...");

    let mut result_val = 0.5;
    let mut success = false;

    // 1. Try Crate (DDC/CI or generic WMI)
    let devices = brightness::brightness_devices()
        .try_collect::<Vec<_>>()
        .await;

    if let Ok(devices) = devices {
        if !devices.is_empty() {
            let mut total = 0.0;
            let mut count = 0;
            let mut success_read = false;

            for (i, device) in devices.iter().enumerate() {
                match device.get().await {
                    Ok(val) => {
                        println!("DEBUG: Device {} value: {}", i, val);
                        total += val as f32;
                        count += 1;
                        success_read = true;
                    }
                    Err(e) => {
                        println!("DEBUG: Device {} read failed: {}", i, e);
                    }
                }
            }
            if success_read && count > 0 {
                result_val = (total / count as f32) / 100.0;
                success = true;
            }
        }
    }

    if !success {
        println!("DEBUG: Crate failed to read. Trying PowerShell WMI fallback...");

        let output = tokio::task::spawn_blocking(move || {
            Command::new("powershell")
                .args(&[
                    "-NoProfile",
                    "-Command",
                    "(Get-CimInstance -Namespace root/wmi -ClassName WmiMonitorBrightness).CurrentBrightness"
                ])
                .creation_flags(CREATE_NO_WINDOW)
                .output()
        }).await
        .map_err(|e| format!("JoinError: {}", e))?
        .map_err(|e| format!("PowerShell exec failed: {}", e))?;

        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let mut total = 0.0;
            let mut count = 0;
            for line in stdout.lines() {
                if let Ok(val) = line.trim().parse::<f32>() {
                    total += val;
                    count += 1;
                }
            }
            if count > 0 {
                result_val = (total / count as f32) / 100.0;
            }
        }
    }

    // Update cache regardless of success to prevent spamming
    // If failed, we cache the default (or old) value for the duration too
    {
        let mut cache = BRIGHTNESS_CACHE.lock().unwrap();
        *cache = (result_val, Some(Instant::now()));
    }

    Ok(result_val)
}

pub async fn set_brightness(val: f32) -> Result<(), String> {
    println!("DEBUG: Setting brightness to {} via Crate...", val);
    let target_val = (val * 100.0) as u32;

    // Update cache immediately to prevent "jump back" on UI
    {
        let mut cache = BRIGHTNESS_CACHE.lock().unwrap();
        *cache = (val, Some(Instant::now()));
    }

    let devices = brightness::brightness_devices()
        .try_collect::<Vec<_>>()
        .await;
    let mut crate_success = false;

    if let Ok(devices) = devices {
        for mut device in devices {
            if device.set(target_val).await.is_ok() {
                crate_success = true;
            }
        }
    }

    if crate_success {
        return Ok(());
    }

    let cmd = format!(
        "(Get-WmiObject -Namespace root/wmi -Class WmiMonitorBrightnessMethods).WmiSetBrightness(1, {})",
        target_val
    );

    let output = tokio::task::spawn_blocking(move || {
        Command::new("powershell")
            .args(&["-NoProfile", "-Command", &cmd])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
    })
    .await
    .map_err(|e| format!("JoinError: {}", e))?
    .map_err(|e| format!("PowerShell exec failed: {}", e))?;

    if output.status.success() {
        Ok(())
    } else {
        Err("PowerShell WMI Set failed".into())
    }
}
