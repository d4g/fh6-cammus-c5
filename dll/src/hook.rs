//! The in-process hook. When `pid.dll` (Microsoft's generic HID PID force-feedback
//! driver) is mapped, the slot for `IDirectInputEffectDriver::DownloadEffect` in its
//! driver vtable is pointed at [`download_effect`], which runs [`repair::repair`]
//! on the effect before forwarding it. Nothing on disk is modified.
//!
//! Why a vtable slot and not a code detour: the vtable sits in `pid.dll`'s writable
//! `.data`, ffbroker.dll calls through it (slot 9), and one pointer write needs no
//! trampoline and no hard-coded code offset. See docs/how-it-works.md in the repository root.

use crate::repair;
use crate::sys::*;
use std::ffi::c_void;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::ptr;
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering::Relaxed};
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

/// `pid.dll`'s COM class (the `OEMForceFeedback` CLSID DirectInput lists for the base).
const CLSID_PID: Guid = Guid { data1: 0xEEC6993A, data2: 0xB3FD, data3: 0x11D2, data4: [0xA9, 0x16, 0x00, 0xC0, 0x4F, 0xB9, 0x86, 0x38] };
const IID_ICLASSFACTORY: Guid = Guid { data1: 1, data2: 0, data3: 0, data4: [0xC0, 0, 0, 0, 0, 0, 0, 0x46] };
const IID_EFFECT_DRIVER: Guid = Guid { data1: 0x02538130, data2: 0x898F, data3: 0x11D0, data4: [0x9A, 0xD0, 0x00, 0xA0, 0xC9, 0xA0, 0x6E, 0x35] };

/// `IDirectInputEffectDriver` vtable order (dinputd.h): IUnknown 0-2, then these.
const SLOT_DEVICE_ID: usize = 3;
const SLOT_SET_GAIN: usize = 6;
const SLOT_SEND_COMMAND: usize = 7;
const SLOT_GET_STATE: usize = 8;
const SLOT_DOWNLOAD_EFFECT: usize = 9;
const SLOT_DESTROY_EFFECT: usize = 10;
const SLOT_START_EFFECT: usize = 11;
const SLOT_STOP_EFFECT: usize = 12;
const SLOT_GET_EFFECT_STATUS: usize = 13;

/// Lines of call logging per process (every failure is logged regardless). The two
/// polling calls (state, effect status) are logged only when their result changes.
const LOG_BUDGET: u32 = 3000;
/// FH6 updates every effect once per frame; only every n-th update is logged.
const LOG_EVERY_NTH_UPDATE: u32 = 100;
const LOG_EVERY_NTH_SKIP: u32 = 1000;
/// `DIEP_START`: a download with this flag also (re)starts the effect, so it is never skipped.
const DIEP_START: u32 = 0x2000_0000;

const DISFFC_RESET: u32 = 0x01;
const DISFFC_SETACTUATORSON: u32 = 0x10;
const DISFFC_SETACTUATORSOFF: u32 = 0x20;

/// `DIEFFECT` as laid out on x64 (80 bytes). Callers built against older headers
/// pass a shorter struct without `dwStartDelay`; `dw_size` tells which.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct DiEffect {
    pub dw_size: u32,
    pub dw_flags: u32,
    pub dw_duration: u32,
    pub dw_sample_period: u32,
    pub dw_gain: u32,
    pub dw_trigger_button: u32,
    pub dw_trigger_repeat_interval: u32,
    pub c_axes: u32,
    pub rgdw_axes: *mut u32,
    pub rgl_direction: *mut i32,
    pub lp_envelope: *mut c_void,
    pub cb_type_specific_params: u32,
    pub lpv_type_specific_params: *mut c_void,
    pub dw_start_delay: u32,
}

type DownloadEffectFn = unsafe extern "system" fn(*mut c_void, u32, u32, *mut u32, *const DiEffect, u32) -> i32;

static ORIGINALS: [AtomicUsize; 14] = [const { AtomicUsize::new(0) }; 14];
static LAST_STATE: AtomicU32 = AtomicU32::new(u32::MAX);
static UPDATES: AtomicU32 = AtomicU32::new(0);
static SKIPPED: AtomicU32 = AtomicU32::new(0);
/// Parameters of the last successful download per (device, effect id), see
/// [`fingerprint`]. A handful of effects per device, so a Vec is enough.
static LAST_DOWNLOADED: Mutex<Vec<((u32, u32), Vec<u8>)>> = Mutex::new(Vec::new());
/// Bit n set: device n's actuators were switched off (by DISFFC_RESET or
/// DISFFC_SETACTUATORSOFF) and nobody has switched them on since.
static ACTUATORS_OFF: AtomicU64 = AtomicU64::new(0);
static LAST_STATUS: Mutex<Vec<(u32, u32, i32, u32)>> = Mutex::new(Vec::new());
/// Base address of the `pid.dll` image that is currently patched (0 = none).
static PATCHED_BASE: AtomicUsize = AtomicUsize::new(0);
static CALLS: AtomicU32 = AtomicU32::new(0);
static LOG: Mutex<Option<std::fs::File>> = Mutex::new(None);
static START: OnceLock<Instant> = OnceLock::new();

// Optional behaviour is switched by marker files next to the game's exe, read once per
// session, so it can change between two game starts without a rebuild.

/// Skip effect updates identical to the last one sent (see [`fingerprint`]). Off by
/// default: FFB is stable without it (2026-10-23); it only reduces USB traffic.
const DEDUPE_MARKER: &str = "fh6-ffb-fix.dedupe";
/// Log every driver call (up to [`LOG_BUDGET`] lines) instead of only the patch, the
/// actuator switch and failures.
const VERBOSE_MARKER: &str = "fh6-ffb-fix.verbose";

fn marker_present(name: &str) -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(name).exists()))
        .unwrap_or(false)
}

fn dedupe_updates() -> bool {
    static DEDUPE: OnceLock<bool> = OnceLock::new();
    *DEDUPE.get_or_init(|| {
        let on = marker_present(DEDUPE_MARKER);
        log(&format!("identical effect updates: {}", if on { "SKIPPED (dedupe marker present)" } else { "sent" }));
        on
    })
}

fn verbose() -> bool {
    static VERBOSE: OnceLock<bool> = OnceLock::new();
    *VERBOSE.get_or_init(|| {
        let on = marker_present(VERBOSE_MARKER);
        if on {
            log("verbose logging (verbose marker present)");
        }
        on
    })
}

pub fn log(msg: &str) {
    use std::io::Write;
    let mut guard = LOG.lock().unwrap_or_else(|e| e.into_inner());
    if guard.is_none() {
        let dir = std::env::var_os("TEMP").map(std::path::PathBuf::from).unwrap_or_default();
        *guard = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("fh6-ffb-fix.log")).ok();
    }
    if let Some(f) = guard.as_mut() {
        let t = START.get_or_init(Instant::now).elapsed().as_millis();
        let _ = writeln!(f, "[{t:>8} ms] {msg}");
    }
}

/// Called from `DllMain`. Registers for module-load events and handles a `pid.dll`
/// that is already mapped.
pub fn init() {
    START.get_or_init(Instant::now);
    let mut cookie = ptr::null_mut();
    let status = unsafe { LdrRegisterDllNotification(0, on_dll_event, ptr::null_mut(), &mut cookie) };
    if status < 0 {
        log(&format!("LdrRegisterDllNotification failed: {status:#010x}"));
    }
    let existing = unsafe { GetModuleHandleW(wide("pid.dll").as_ptr()) };
    if !existing.is_null() {
        spawn_install(existing as usize);
    }
}

/// Runs under the loader lock, so it only starts a thread; the thread's own start-up
/// waits for the lock, which guarantees `pid.dll`'s `DllMain` has finished by then.
unsafe extern "system" fn on_dll_event(reason: u32, data: *const LdrDllNotificationData, _ctx: *mut c_void) {
    let _ = catch_unwind(|| {
        let d = &*data;
        if d.base_dll_name.is_null() {
            return;
        }
        let n = &*d.base_dll_name;
        let name = String::from_utf16_lossy(std::slice::from_raw_parts(n.buffer, n.length as usize / 2));
        if !name.eq_ignore_ascii_case("pid.dll") {
            return;
        }
        match reason {
            LDR_DLL_NOTIFICATION_REASON_LOADED => spawn_install(d.dll_base as usize),
            // A later load maps a fresh, unpatched vtable.
            LDR_DLL_NOTIFICATION_REASON_UNLOADED => PATCHED_BASE.store(0, Relaxed),
            _ => {}
        }
    });
}

fn spawn_install(base: usize) {
    let _ = std::thread::Builder::new().name("fh6-ffb-fix".into()).spawn(move || {
        match catch_unwind(|| unsafe { install(base) }) {
            Ok(Ok(msg)) => log(&msg),
            Ok(Err(e)) => log(&format!("install failed: {e}")),
            Err(_) => log("install panicked"),
        }
    });
}

unsafe fn image_size(base: usize) -> Result<usize, String> {
    if *(base as *const u16) != 0x5A4D {
        return Err("no MZ header".into());
    }
    let pe = base + *((base + 0x3C) as *const u32) as usize;
    if *(pe as *const u32) != 0x0000_4550 {
        return Err("no PE signature".into());
    }
    // OptionalHeader starts at pe+24; SizeOfImage is at +56 in PE32 and PE32+.
    Ok(*((pe + 24 + 56) as *const u32) as usize)
}

unsafe fn release(obj: *mut c_void) {
    let vtbl = *(obj as *const *const usize);
    let release: unsafe extern "system" fn(*mut c_void) -> u32 = std::mem::transmute(*vtbl.add(2));
    release(obj);
}

/// Creates one driver instance only to read the shared vtable pointer, then swaps
/// the `DownloadEffect` slot. The slot and the original target must both lie inside
/// the `pid.dll` image, otherwise nothing is written.
unsafe fn install(base: usize) -> Result<String, String> {
    if PATCHED_BASE.load(Relaxed) == base {
        return Ok("pid.dll already patched".into());
    }
    let size = image_size(base)?;
    let inside = |p: usize| p >= base && p < base + size;

    let gco = GetProcAddress(base as *mut c_void, c"DllGetClassObject".as_ptr().cast());
    if gco.is_null() {
        return Err("pid.dll exports no DllGetClassObject".into());
    }
    let gco: unsafe extern "system" fn(*const Guid, *const Guid, *mut *mut c_void) -> i32 = std::mem::transmute(gco);
    let mut factory = ptr::null_mut();
    let hr = gco(&CLSID_PID, &IID_ICLASSFACTORY, &mut factory);
    if hr < 0 || factory.is_null() {
        return Err(format!("DllGetClassObject: {hr:#010x}"));
    }

    // IClassFactory: QueryInterface, AddRef, Release, CreateInstance, LockServer.
    let fvtbl = *(factory as *const *const usize);
    let create: unsafe extern "system" fn(*mut c_void, *mut c_void, *const Guid, *mut *mut c_void) -> i32 =
        std::mem::transmute(*fvtbl.add(3));
    let mut obj = ptr::null_mut();
    let hr = create(factory, ptr::null_mut(), &IID_EFFECT_DRIVER, &mut obj);
    release(factory);
    if hr < 0 || obj.is_null() {
        return Err(format!("CreateInstance(IDirectInputEffectDriver): {hr:#010x}"));
    }

    let vtbl = *(obj as *const *mut usize);
    // DownloadEffect carries the repair; the others only log, to see what happens
    // after a successful download.
    let hooks: [(usize, usize); 9] = [
        (SLOT_DOWNLOAD_EFFECT, download_effect as *const () as usize),
        (SLOT_DEVICE_ID, device_id as *const () as usize),
        (SLOT_SET_GAIN, set_gain as *const () as usize),
        (SLOT_SEND_COMMAND, send_command as *const () as usize),
        (SLOT_GET_STATE, get_state as *const () as usize),
        (SLOT_DESTROY_EFFECT, destroy_effect as *const () as usize),
        (SLOT_START_EFFECT, start_effect as *const () as usize),
        (SLOT_STOP_EFFECT, stop_effect as *const () as usize),
        (SLOT_GET_EFFECT_STATUS, get_effect_status as *const () as usize),
    ];
    let result = (|| {
        // Validate every slot before writing any, so a mismatch leaves pid.dll untouched.
        for &(i, wrapper) in &hooks {
            let slot = vtbl.add(i);
            let original = *slot;
            if !inside(slot as usize) || (!inside(original) && original != wrapper) {
                return Err(format!(
                    "vtable layout not as expected (slot {i} at {:#x}, target {original:#x}, image {base:#x}+{size:#x})",
                    slot as usize
                ));
            }
        }
        let first = vtbl.add(SLOT_DEVICE_ID);
        let span = (SLOT_GET_EFFECT_STATUS - SLOT_DEVICE_ID + 1) * 8;
        let mut old = 0;
        if VirtualProtect(first.cast(), span, PAGE_READWRITE, &mut old) == 0 {
            return Err("VirtualProtect failed".into());
        }
        for &(i, wrapper) in &hooks {
            let slot = vtbl.add(i);
            if *slot != wrapper {
                ORIGINALS[i].store(*slot, Relaxed);
                *slot = wrapper;
            }
        }
        VirtualProtect(first.cast(), span, old, &mut old);
        PATCHED_BASE.store(base, Relaxed);
        Ok(format!(
            "patched pid.dll {base:#x}: vtable {:#x} (+{:#x}), DownloadEffect was {:#x} (+{:#x}); slots 3,6-13 wrapped",
            vtbl as usize,
            vtbl as usize - base,
            ORIGINALS[SLOT_DOWNLOAD_EFFECT].load(Relaxed),
            ORIGINALS[SLOT_DOWNLOAD_EFFECT].load(Relaxed) - base
        ))
    })();
    release(obj);
    result
}

/// A repaired copy of an effect. The vectors back the pointers in `eff` and must
/// outlive the call into pid.dll.
struct Patched {
    eff: DiEffect,
    _axes: Vec<u32>,
    _dirs: Option<Vec<i32>>,
    _type_specific: Option<Vec<u8>>,
    note: String,
}

/// # Safety
/// `peff` must be null or point to a `DIEFFECT` whose pointer members are valid for
/// `cAxes` / `cbTypeSpecificParams` entries, as pid.dll itself would assume.
unsafe fn prepare(peff: *const DiEffect) -> Option<Patched> {
    // Through lpvTypeSpecificParams: the part every DIEFFECT version has.
    const MIN_SIZE: usize = 72;
    if peff.is_null() || ((*peff).dw_size as usize) < MIN_SIZE {
        return None;
    }
    let e = &*peff;
    if e.c_axes == 0 || e.c_axes > 32 || e.rgdw_axes.is_null() {
        return None;
    }
    let n = e.c_axes as usize;
    let axes = std::slice::from_raw_parts(e.rgdw_axes, n);
    let dirs = (!e.rgl_direction.is_null()).then(|| std::slice::from_raw_parts(e.rgl_direction, n));
    let ts = (!e.lpv_type_specific_params.is_null() && e.cb_type_specific_params > 0)
        .then(|| std::slice::from_raw_parts(e.lpv_type_specific_params as *const u8, e.cb_type_specific_params as usize));

    let r = repair::repair(e.dw_flags, axes, dirs, ts)?;

    let mut eff = DiEffect { dw_start_delay: 0, ..*e };
    // Copy only what the caller's struct actually has.
    let have = (e.dw_size as usize).min(std::mem::size_of::<DiEffect>());
    ptr::copy_nonoverlapping(peff as *const u8, &mut eff as *mut DiEffect as *mut u8, have);

    let mut new_axes = r.axes.clone();
    let mut new_dirs = r.dirs.clone();
    let mut new_ts = r.type_specific.clone();
    eff.dw_flags = r.flags;
    eff.c_axes = new_axes.len() as u32;
    eff.rgdw_axes = new_axes.as_mut_ptr();
    if let Some(d) = new_dirs.as_mut() {
        eff.rgl_direction = d.as_mut_ptr();
    }
    if let Some(t) = new_ts.as_mut() {
        eff.lpv_type_specific_params = t.as_mut_ptr().cast();
        eff.cb_type_specific_params = t.len() as u32;
    }
    let note = format!(
        "axes {axes:x?} dirs {dirs:?} flags {:#x} -> axes {:x?} dirs {:?} flags {:#x} (components {:.3?})",
        e.dw_flags, r.axes, new_dirs, r.flags, r.components
    );
    Some(Patched { eff, _axes: new_axes, _dirs: new_dirs, _type_specific: new_ts, note })
}

unsafe extern "system" fn download_effect(
    this: *mut c_void,
    device_id: u32,
    effect_type: u32,
    download_id: *mut u32,
    peff: *const DiEffect,
    flags: u32,
) -> i32 {
    let original: DownloadEffectFn = std::mem::transmute(ORIGINALS[SLOT_DOWNLOAD_EFFECT].load(Relaxed));
    let patched = catch_unwind(AssertUnwindSafe(|| prepare(peff))).ok().flatten();
    let forwarded = patched.as_ref().map_or(peff, |p| &p.eff as *const DiEffect);

    let id_in = if download_id.is_null() { 0 } else { *download_id };
    if id_in != 0 {
        actuators_on_if_off(this, device_id, &format!("first update of effect {id_in}"));
    }

    let print = if dedupe_updates() {
        catch_unwind(|| fingerprint(effect_type, flags, forwarded)).ok().flatten()
    } else {
        None
    };
    if id_in != 0 && flags & DIEP_START == 0 {
        if let Some(p) = &print {
            if last_download_matches(device_id, id_in, p) {
                let n = SKIPPED.fetch_add(1, Relaxed) + 1;
                if n % LOG_EVERY_NTH_SKIP == 0 && should_log(0) {
                    log(&format!("skipped {n} identical effect updates so far"));
                }
                return 0; // S_OK: the device already holds exactly these parameters
            }
        }
    }

    let hr = original(this, device_id, effect_type, download_id, forwarded, flags);
    let id_out = if download_id.is_null() { 0 } else { *download_id };
    remember_download(device_id, id_out, if hr >= 0 { print } else { None });

    let sampled = id_in == 0 || UPDATES.fetch_add(1, Relaxed) % LOG_EVERY_NTH_UPDATE == 0;
    if (sampled || hr < 0) && should_log(hr) {
        let what = patched.as_ref().map_or("unchanged".to_string(), |p| p.note.clone());
        let params = catch_unwind(|| describe_params(forwarded)).unwrap_or_default();
        log(&format!(
            "DownloadEffect dev {device_id} type {effect_type:#x} id {id_in}->{id_out} flags {flags:#x} -> hr {hr:#010x}; {params}; {what}"
        ));
    }
    hr
}

/// Type-specific parameters, told apart by size: DICONSTANTFORCE (4 bytes),
/// DIRAMPFORCE (8), DIPERIODIC (16), one DICONDITION per 24 bytes.
unsafe fn describe_params(peff: *const DiEffect) -> String {
    if peff.is_null() {
        return String::new();
    }
    let e = &*peff;
    let gain = e.dw_gain;
    let p = e.lpv_type_specific_params as *const i32;
    if p.is_null() {
        return format!("gain {gain}, no params");
    }
    let v = std::slice::from_raw_parts(p, e.cb_type_specific_params as usize / 4);
    match e.cb_type_specific_params {
        4 => format!("gain {gain}, constant magnitude {}", v[0]),
        8 => format!("gain {gain}, ramp {} -> {}", v[0], v[1]),
        16 => format!("gain {gain}, periodic magnitude {} offset {} phase {} period {} us", v[0] as u32, v[1], v[2] as u32, v[3] as u32),
        n if n % 24 == 0 => {
            let c: Vec<String> = v
                .chunks(6)
                .map(|c| format!("offset {} coef +{}/-{} sat +{}/-{} dead {}", c[0], c[1], c[2], c[3] as u32, c[4] as u32, c[5]))
                .collect();
            format!("gain {gain}, condition [{}]", c.join("; "))
        }
        n => format!("gain {gain}, {n} bytes of params"),
    }
}

/// Everything a download sends to the device: the call's type and flags, the scalar
/// DIEFFECT members, and the contents (not addresses) of axes, directions, envelope
/// and type-specific parameters. Two downloads with the same fingerprint for the same
/// effect leave the device in the same state.
///
/// Why skip at all: FH6 re-sends all of its effects every frame, also the unchanged
/// ones (two sines at gain 0, usually spring and damper). Each download is a blocking
/// HID write of about 2 ms on the C5, so 7 per 16 ms frame keep the line nearly busy,
/// and the constant force, sent last, reaches the wheel late. Measured 2026-10-23.
unsafe fn fingerprint(effect_type: u32, flags: u32, peff: *const DiEffect) -> Option<Vec<u8>> {
    if peff.is_null() {
        return None;
    }
    let e = &*peff;
    let mut out = Vec::with_capacity(128);
    for v in [effect_type, flags, e.dw_flags, e.dw_duration, e.dw_sample_period, e.dw_gain, e.dw_trigger_button,
        e.dw_trigger_repeat_interval, e.c_axes, e.cb_type_specific_params, e.dw_start_delay]
    {
        out.extend_from_slice(&v.to_le_bytes());
    }
    let n = e.c_axes as usize;
    if n > 32 {
        return None;
    }
    if !e.rgdw_axes.is_null() {
        std::slice::from_raw_parts(e.rgdw_axes, n).iter().for_each(|v| out.extend_from_slice(&v.to_le_bytes()));
    }
    out.push(0xAA); // separator: axes and directions have the same length
    if !e.rgl_direction.is_null() {
        std::slice::from_raw_parts(e.rgl_direction, n).iter().for_each(|v| out.extend_from_slice(&v.to_le_bytes()));
    }
    out.push(0xBB);
    if !e.lp_envelope.is_null() {
        // DIENVELOPE: dwSize, dwAttackLevel, dwAttackTime, dwFadeLevel, dwFadeTime.
        out.extend_from_slice(std::slice::from_raw_parts(e.lp_envelope as *const u8, 20));
    }
    out.push(0xCC);
    if !e.lpv_type_specific_params.is_null() {
        out.extend_from_slice(std::slice::from_raw_parts(
            e.lpv_type_specific_params as *const u8,
            e.cb_type_specific_params as usize,
        ));
    }
    Some(out)
}

fn last_download_matches(dev: u32, id: u32, print: &[u8]) -> bool {
    let last = LAST_DOWNLOADED.lock().unwrap_or_else(|e| e.into_inner());
    last.iter().any(|(k, v)| *k == (dev, id) && v == print)
}

/// Records the parameters of a successful download; `None` forgets the effect, so
/// the next download of it always reaches the device.
fn remember_download(dev: u32, id: u32, print: Option<Vec<u8>>) {
    if id == 0 {
        return;
    }
    let mut last = LAST_DOWNLOADED.lock().unwrap_or_else(|e| e.into_inner());
    last.retain(|(k, _)| *k != (dev, id));
    if let Some(p) = print {
        last.push(((dev, id), p));
    }
}

fn forget_device(dev: u32) {
    LAST_DOWNLOADED.lock().unwrap_or_else(|e| e.into_inner()).retain(|((d, _), _)| *d != dev);
}

/// Counts against [`LOG_BUDGET`]; failures are always logged.
fn should_log(hr: i32) -> bool {
    hr < 0 || (verbose() && CALLS.fetch_add(1, Relaxed) < LOG_BUDGET)
}

/// The original function of a vtable slot, typed by the caller.
macro_rules! original {
    ($slot:expr, fn($($t:ty),*)) => {
        std::mem::transmute::<usize, unsafe extern "system" fn(*mut c_void, $($t),*) -> i32>(ORIGINALS[$slot].load(Relaxed))
    };
}

unsafe extern "system" fn device_id(this: *mut c_void, di_version: u32, external_id: u32, begin: u32, internal_id: u32, reserved: *mut c_void) -> i32 {
    let hr = original!(SLOT_DEVICE_ID, fn(u32, u32, u32, u32, *mut c_void))(this, di_version, external_id, begin, internal_id, reserved);
    if should_log(hr) {
        log(&format!("DeviceID version {di_version:#x} external {external_id} begin {begin} internal {internal_id} -> hr {hr:#010x}"));
    }
    hr
}

unsafe extern "system" fn set_gain(this: *mut c_void, dev: u32, gain: u32) -> i32 {
    let hr = original!(SLOT_SET_GAIN, fn(u32, u32))(this, dev, gain);
    if should_log(hr) {
        log(&format!("SetGain dev {dev} gain {gain} -> hr {hr:#010x}"));
    }
    hr
}

/// `DISFFC_*`: 0x1 RESET, 0x2 STOPALL, 0x4 PAUSE, 0x8 CONTINUE, 0x10 SETACTUATORSON, 0x20 SETACTUATORSOFF.
unsafe extern "system" fn send_command(this: *mut c_void, dev: u32, command: u32) -> i32 {
    let hr = original!(SLOT_SEND_COMMAND, fn(u32, u32))(this, dev, command);
    if command & DISFFC_RESET != 0 {
        forget_device(dev); // RESET removes all effects from the device
    }
    if hr >= 0 && dev < 64 {
        if command & (DISFFC_RESET | DISFFC_SETACTUATORSOFF) != 0 {
            ACTUATORS_OFF.fetch_or(1 << dev, Relaxed);
        }
        if command & DISFFC_SETACTUATORSON != 0 {
            ACTUATORS_OFF.fetch_and(!(1 << dev), Relaxed);
        }
    }
    if should_log(hr) {
        log(&format!("SendForceFeedbackCommand dev {dev} command {command:#x} -> hr {hr:#010x}"));
    }
    hr
}

/// `DIDEVICESTATE`: dwSize, dwState (`DIGFFS_*`), dwLoad.
unsafe extern "system" fn get_state(this: *mut c_void, dev: u32, state: *mut [u32; 3]) -> i32 {
    let hr = original!(SLOT_GET_STATE, fn(u32, *mut [u32; 3]))(this, dev, state);
    let value = if hr >= 0 && !state.is_null() { (*state)[1] } else { u32::MAX - 1 };
    if (LAST_STATE.swap(value, Relaxed) != value || hr < 0) && should_log(hr) {
        let load = if state.is_null() { 0 } else { (*state)[2] };
        log(&format!("GetForceFeedbackState dev {dev} -> hr {hr:#010x} state {value:#x} load {load} (logged on change)"));
    }
    hr
}

unsafe extern "system" fn destroy_effect(this: *mut c_void, dev: u32, id: u32) -> i32 {
    let hr = original!(SLOT_DESTROY_EFFECT, fn(u32, u32))(this, dev, id);
    remember_download(dev, id, None);
    if should_log(hr) {
        log(&format!("DestroyEffect dev {dev} id {id} -> hr {hr:#010x}"));
    }
    hr
}

/// Windows.Gaming.Input sends DISFFC_RESET ("the device's actuators are disabled",
/// per the DirectInput docs) and DISFFC_SETACTUATORSOFF during setup, then starts
/// effects without ever sending DISFFC_SETACTUATORSON, so every effect plays muted.
///
/// The actuators are switched on at the first *update* of an effect, not at the
/// first start: setup starts the effects even when FH6 never drives them (its FFB
/// device is another controller), and while the actuators stay off the C5 firmware
/// keeps its own centring spring. Observed 2026-10-23.
unsafe fn actuators_on_if_off(this: *mut c_void, dev: u32, why: &str) {
    if dev < 64 && ACTUATORS_OFF.load(Relaxed) & (1 << dev) != 0 {
        let hr = original!(SLOT_SEND_COMMAND, fn(u32, u32))(this, dev, DISFFC_SETACTUATORSON);
        if hr >= 0 {
            ACTUATORS_OFF.fetch_and(!(1 << dev), Relaxed);
        }
        log(&format!("dev {dev}: actuators were off, sent SETACTUATORSON before {why} -> hr {hr:#010x}"));
    }
}

unsafe extern "system" fn start_effect(this: *mut c_void, dev: u32, id: u32, mode: u32, count: u32) -> i32 {
    let hr = original!(SLOT_START_EFFECT, fn(u32, u32, u32, u32))(this, dev, id, mode, count);
    if should_log(hr) {
        log(&format!("StartEffect dev {dev} id {id} mode {mode:#x} count {count} -> hr {hr:#010x}"));
    }
    hr
}

unsafe extern "system" fn stop_effect(this: *mut c_void, dev: u32, id: u32) -> i32 {
    let hr = original!(SLOT_STOP_EFFECT, fn(u32, u32))(this, dev, id);
    if should_log(hr) {
        log(&format!("StopEffect dev {dev} id {id} -> hr {hr:#010x}"));
    }
    hr
}

unsafe extern "system" fn get_effect_status(this: *mut c_void, dev: u32, id: u32, status: *mut u32) -> i32 {
    let hr = original!(SLOT_GET_EFFECT_STATUS, fn(u32, u32, *mut u32))(this, dev, id, status);
    let value = if hr >= 0 && !status.is_null() { *status } else { u32::MAX };
    let changed = {
        let mut last = LAST_STATUS.lock().unwrap_or_else(|e| e.into_inner());
        match last.iter_mut().find(|e| e.0 == dev && e.1 == id) {
            Some(e) if (e.2, e.3) == (hr, value) => false,
            Some(e) => {
                (e.2, e.3) = (hr, value);
                true
            }
            None => {
                last.push((dev, id, hr, value));
                true
            }
        }
    };
    if (changed || hr < 0) && should_log(hr) {
        log(&format!("GetEffectStatus dev {dev} id {id} -> hr {hr:#010x} status {value:#x} (logged on change)"));
    }
    hr
}

#[cfg(test)]
mod tests {
    use super::*;

    fn effect(flags: u32, axes: &mut [u32], dirs: &mut [i32]) -> DiEffect {
        DiEffect {
            dw_size: std::mem::size_of::<DiEffect>() as u32,
            dw_flags: flags,
            dw_duration: 0,
            dw_sample_period: 0,
            dw_gain: 10000,
            dw_trigger_button: 0xFFFF_FFFF,
            dw_trigger_repeat_interval: 0,
            c_axes: axes.len() as u32,
            rgdw_axes: axes.as_mut_ptr(),
            rgl_direction: dirs.as_mut_ptr(),
            lp_envelope: ptr::null_mut(),
            cb_type_specific_params: 0,
            lpv_type_specific_params: ptr::null_mut(),
            dw_start_delay: 0,
        }
    }

    #[test]
    fn struct_matches_the_x64_dieffect_layout() {
        assert_eq!(std::mem::size_of::<DiEffect>(), 80);
    }

    #[test]
    fn polar_x_plus_z_becomes_single_axis_cartesian() {
        let mut axes = [0x0100_0002, 0x0000_0702];
        let mut dirs = [9000, 0];
        let e = effect(repair::DIEFF_POLAR, &mut axes, &mut dirs);
        let p = unsafe { prepare(&e) }.expect("repaired");
        assert_eq!(p.eff.c_axes, 1);
        assert_eq!(unsafe { *p.eff.rgdw_axes }, 0x0100_0002);
        assert_eq!(unsafe { *p.eff.rgl_direction }, 1);
        assert_eq!(p.eff.dw_flags, repair::DIEFF_CARTESIAN);
        assert_eq!(p.eff.dw_gain, 10000, "untouched fields are copied");
        assert_eq!(e.c_axes, 2, "the caller's struct is not modified");
    }

    #[test]
    #[allow(unused_assignments)] // m2/d2 are read through the effect's raw pointers
    fn fingerprint_compares_contents_not_addresses() {
        let (mut a1, mut d1, mut m1) = ([0x0100_0002u32], [-1i32], [1433i32]);
        let (mut a2, mut d2, mut m2) = ([0x0100_0002u32], [-1i32], [1433i32]);
        let mut e1 = effect(repair::DIEFF_CARTESIAN, &mut a1, &mut d1);
        let mut e2 = effect(repair::DIEFF_CARTESIAN, &mut a2, &mut d2);
        for (e, m) in [(&mut e1, &mut m1), (&mut e2, &mut m2)] {
            e.cb_type_specific_params = 4;
            e.lpv_type_specific_params = m.as_mut_ptr().cast();
        }
        let f = |e: &DiEffect| unsafe { fingerprint(0xf0026, 0x3ff, e) }.unwrap();
        assert_eq!(f(&e1), f(&e2), "same parameters at different addresses");
        m2[0] = -827;
        assert_ne!(f(&e1), f(&e2), "a new magnitude must reach the device");
        m2[0] = 1433;
        d2[0] = 1;
        assert_ne!(f(&e1), f(&e2), "a new direction must reach the device");
    }

    #[test]
    fn remembered_downloads_are_forgotten_on_destroy_and_reset() {
        remember_download(7, 1, Some(vec![1, 2, 3]));
        remember_download(7, 2, Some(vec![4]));
        assert!(last_download_matches(7, 1, &[1, 2, 3]));
        remember_download(7, 1, None);
        assert!(!last_download_matches(7, 1, &[1, 2, 3]));
        forget_device(7);
        assert!(!last_download_matches(7, 2, &[4]));
    }

    #[test]
    fn single_actuator_effects_are_forwarded_as_is() {
        let mut axes = [0x0100_0002];
        let mut dirs = [1];
        let e = effect(repair::DIEFF_CARTESIAN, &mut axes, &mut dirs);
        assert!(unsafe { prepare(&e) }.is_none());
        assert!(unsafe { prepare(ptr::null()) }.is_none());
    }
}
