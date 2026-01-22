use std::ffi::c_void;
use windows::core::Result;
use windows::Win32::UI::WindowsAndMessaging::{
    SystemParametersInfoA, SPIF_SENDCHANGE, SPIF_UPDATEINIFILE, SPI_GETMOUSESPEED,
    SPI_SETMOUSESPEED,
};

pub fn get_mouse_sensitivity() -> Result<u32> {
    unsafe {
        let mut speed: u32 = 0;
        SystemParametersInfoA(
            SPI_GETMOUSESPEED,
            0,
            Some(&mut speed as *mut _ as *mut c_void),
            windows::Win32::UI::WindowsAndMessaging::SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )?;
        Ok(speed)
    }
}

pub fn set_mouse_sensitivity(val: u32) -> Result<()> {
    let val = val.clamp(1, 20);
    unsafe {
        let _ = SystemParametersInfoA(
            SPI_SETMOUSESPEED,
            0,
            Some(val as *mut c_void),
            SPIF_UPDATEINIFILE | SPIF_SENDCHANGE,
        );
        Ok(())
    }
}
