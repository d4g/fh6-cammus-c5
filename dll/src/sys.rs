//! The few Win32 / NT entry points the proxy needs, declared by hand so the crate
//! has no dependencies (nothing to audit or pin besides rustc itself).

use std::ffi::c_void;

#[repr(C)]
pub struct Guid {
    pub data1: u32,
    pub data2: u16,
    pub data3: u16,
    pub data4: [u8; 8],
}

#[repr(C)]
pub struct UnicodeString {
    pub length: u16,
    pub maximum_length: u16,
    pub buffer: *const u16,
}

/// `LDR_DLL_LOADED_NOTIFICATION_DATA`; the UNLOADED variant has the same layout.
#[repr(C)]
pub struct LdrDllNotificationData {
    pub flags: u32,
    pub full_dll_name: *const UnicodeString,
    pub base_dll_name: *const UnicodeString,
    pub dll_base: *mut c_void,
    pub size_of_image: u32,
}

pub const LDR_DLL_NOTIFICATION_REASON_LOADED: u32 = 1;
pub const LDR_DLL_NOTIFICATION_REASON_UNLOADED: u32 = 2;
pub const PAGE_READWRITE: u32 = 0x04;

pub type LdrNotification = unsafe extern "system" fn(u32, *const LdrDllNotificationData, *mut c_void);

#[link(name = "kernel32")]
extern "system" {
    pub fn GetModuleHandleW(name: *const u16) -> *mut c_void;
    /// `name` is a NUL-terminated ANSI string, or an ordinal in the low word.
    pub fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
    pub fn LoadLibraryW(path: *const u16) -> *mut c_void;
    pub fn GetSystemDirectoryW(buf: *mut u16, size: u32) -> u32;
    pub fn VirtualProtect(addr: *mut c_void, size: usize, new: u32, old: *mut u32) -> i32;
    pub fn DisableThreadLibraryCalls(module: *mut c_void) -> i32;
}

#[link(name = "ntdll")]
extern "system" {
    pub fn LdrRegisterDllNotification(
        flags: u32,
        callback: LdrNotification,
        context: *mut c_void,
        cookie: *mut *mut c_void,
    ) -> i32;
}

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}
