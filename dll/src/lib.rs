//! `xinput1_3.dll` proxy for Forza Horizon 6. FH6 imports XINPUT1_3.dll and the game
//! folder is searched before System32, so this DLL is loaded into the game. It
//! forwards every XInput call to the real System32 DLL and, as a side effect of being
//! loaded, installs the force-feedback repair from [`hook`].

#[cfg(not(target_pointer_width = "64"))]
compile_error!("FH6 is x64 only; the DIEFFECT layout and the export forwarding assume it");

pub mod hook;
pub mod repair;
mod sys;

use std::ffi::{c_void, CStr};
use std::sync::OnceLock;
use sys::*;

#[no_mangle]
pub extern "system" fn DllMain(module: *mut c_void, reason: u32, _reserved: *mut c_void) -> i32 {
    const DLL_PROCESS_ATTACH: u32 = 1;
    if reason == DLL_PROCESS_ATTACH {
        unsafe { DisableThreadLibraryCalls(module) };
        let _ = std::panic::catch_unwind(hook::init);
    }
    1
}

enum Sym {
    Name(&'static CStr),
    Ordinal(u16),
}

/// The genuine `xinput1_3.dll`, loaded by full path: a bare name would find this proxy.
fn real_module() -> usize {
    static MODULE: OnceLock<usize> = OnceLock::new();
    *MODULE.get_or_init(|| unsafe {
        let mut dir = [0u16; 260];
        let len = GetSystemDirectoryW(dir.as_mut_ptr(), dir.len() as u32) as usize;
        if len == 0 || len >= dir.len() {
            return 0;
        }
        let mut path = String::from_utf16_lossy(&dir[..len]);
        path.push_str("\\xinput1_3.dll");
        LoadLibraryW(wide(&path).as_ptr()) as usize
    })
}

fn resolve(sym: Sym) -> usize {
    let module = real_module();
    if module == 0 {
        return 0;
    }
    let name = match sym {
        Sym::Name(n) => n.as_ptr().cast(),
        Sym::Ordinal(o) => o as usize as *const u8,
    };
    unsafe { GetProcAddress(module as *mut c_void, name) as usize }
}

const ERROR_DEVICE_NOT_CONNECTED: u32 = 1167;

/// Defines one exported forwarder. The symbol name must match xinput1_3.def.
macro_rules! forward {
    ($sym:expr, fn $name:ident($($arg:ident: $ty:ty),*) -> $ret:ty, missing = $missing:expr) => {
        #[no_mangle]
        pub unsafe extern "system" fn $name($($arg: $ty),*) -> $ret {
            static TARGET: OnceLock<usize> = OnceLock::new();
            let target = *TARGET.get_or_init(|| resolve($sym));
            if target == 0 {
                return $missing;
            }
            let real: unsafe extern "system" fn($($ty),*) -> $ret = std::mem::transmute(target);
            real($($arg),*)
        }
    };
}

forward!(Sym::Name(c"XInputGetState"), fn XInputGetState(index: u32, state: *mut c_void) -> u32, missing = ERROR_DEVICE_NOT_CONNECTED);
forward!(Sym::Name(c"XInputSetState"), fn XInputSetState(index: u32, vibration: *mut c_void) -> u32, missing = ERROR_DEVICE_NOT_CONNECTED);
forward!(Sym::Name(c"XInputGetCapabilities"), fn XInputGetCapabilities(index: u32, flags: u32, caps: *mut c_void) -> u32, missing = ERROR_DEVICE_NOT_CONNECTED);
forward!(Sym::Name(c"XInputEnable"), fn XInputEnable(enable: i32) -> (), missing = ());
forward!(Sym::Name(c"XInputGetDSoundAudioDeviceGuids"), fn XInputGetDSoundAudioDeviceGuids(index: u32, render: *mut c_void, capture: *mut c_void) -> u32, missing = ERROR_DEVICE_NOT_CONNECTED);
forward!(Sym::Name(c"XInputGetBatteryInformation"), fn XInputGetBatteryInformation(index: u32, kind: u8, info: *mut c_void) -> u32, missing = ERROR_DEVICE_NOT_CONNECTED);
forward!(Sym::Name(c"XInputGetKeystroke"), fn XInputGetKeystroke(index: u32, reserved: u32, key: *mut c_void) -> u32, missing = ERROR_DEVICE_NOT_CONNECTED);

// The four name-less exports are undocumented. They are passed all four x64 argument
// registers and a full-width return value, so any real signature up to four
// register-sized arguments is forwarded intact.
forward!(Sym::Ordinal(100), fn XInputGetStateEx(a: usize, b: usize, c: usize, d: usize) -> usize, missing = ERROR_DEVICE_NOT_CONNECTED as usize);
forward!(Sym::Ordinal(101), fn XInputWaitForGuideButton(a: usize, b: usize, c: usize, d: usize) -> usize, missing = ERROR_DEVICE_NOT_CONNECTED as usize);
forward!(Sym::Ordinal(102), fn XInputCancelGuideButtonWait(a: usize, b: usize, c: usize, d: usize) -> usize, missing = ERROR_DEVICE_NOT_CONNECTED as usize);
forward!(Sym::Ordinal(103), fn XInputPowerOffController(a: usize, b: usize, c: usize, d: usize) -> usize, missing = ERROR_DEVICE_NOT_CONNECTED as usize);
