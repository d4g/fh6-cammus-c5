# fh6-ffb-fix

An `xinput1_3.dll` proxy for **Forza Horizon 6** that stops the game from crashing
(`pid.dll+0x35c6`) on a force-feedback wheel whose only DirectInput actuator is X, and
makes its effects play. Written for the Cammus C5 (VID_3416 PID_0301) on Windows 11
10.0.26100; it applies to any HID PID wheel that shows the same failure.

In short: Windows' FFB broker sends FH6's effects as **2-axis polar effects over X + Z** to
`pid.dll`, Microsoft's generic HID PID driver; DirectInput allows only X on a wheel, so
`pid.dll` rejects Z and crashes in its error path. Background:
[docs/how-it-works.md](../docs/how-it-works.md); installation: [README.md](../README.md).

## What the DLL does

1. FH6 imports `XINPUT1_3.dll`, which the game does not ship, so Windows loads this proxy
   from the game folder first. Every XInput export is forwarded to the genuine
   `C:\Windows\System32\xinput1_3.dll`, loaded by full path (a bare name would find the
   proxy again). Controllers keep working.
2. On load it registers a DLL-load notification. When `pid.dll` is mapped, a worker
   thread asks `pid.dll` for one driver instance (`DllGetClassObject` with CLSID
   `{EEC6993A-B3FD-11D2-A916-00C04FB98638}`, then `CreateInstance`) to read the vtable
   shared by all instances, and replaces slot 9 (`DownloadEffect`) with a wrapper. The
   write happens in memory only; no file is touched. Before writing it checks that the
   slot and the original target lie inside the `pid.dll` image, and otherwise logs and
   does nothing. A later reload of `pid.dll` is patched again.
3. The wrapper drops every axis whose object ID has neither `DIDFT_FFACTUATOR`
   (`0x01000000`) nor `DIDFT_FFEFFECTTRIGGER` (`0x02000000`), the exact test `pid.dll` fails
   on, switches the effect to cartesian coordinates and forwards a copy. The caller's
   `DIEFFECT` is never modified. The logic is in [src/repair.rs](src/repair.rs) and has no FFI
   so it is unit-tested.
   - **Direction.** One kept axis: only the sign of the original direction's component
     along it is kept. A component of exactly zero (force entirely along the dropped axis)
     becomes positive, because a one-axis effect has no way to express "nothing" and
     dropping the effect would be a different behaviour. Not verified against how Forza
     expects this to feel. Several kept axes: the components are re-normalised to 10000.
   - **Per-axis condition arrays** (spring, damper, inertia, friction) are cut to the kept
     axes; other type-specific data passes through.
   - Effects that need no change are forwarded untouched.
4. **Actuators.** Windows.Gaming.Input's setup sends `DISFFC_STOPALL`,
   `DISFFC_SETACTUATORSOFF` and `DISFFC_RESET` (which per the DirectInput docs leaves the
   actuators disabled) and never `DISFFC_SETACTUATORSON`, so every effect would play muted.
   The proxy wraps the other driver slots too (3, 6–13: `SendForceFeedbackCommand` for this
   state, the rest for the verbose log) and tracks that state per device; at
   the first *update* of an effect on a device whose actuators are off, it sends
   `DISFFC_SETACTUATORSON` first. Not at the first start: setup starts the effects even when
   FH6 never drives them, and while the actuators stay off the C5 firmware keeps its own
   centring spring.
5. **Switches.** Optional behaviour is turned on by an empty marker file next to
   `forzahorizon6.exe`, read once per game start:

   | Marker | Effect | Default |
   |---|---|---|
   | `fh6-ffb-fix.dedupe` | skip effect updates identical to the last one sent for that effect (FH6 re-sends all seven effects every frame, each a ~2 ms USB write); it still returns `S_OK`, new effects and anything after a reset or destroy always go through | off: FFB was stable without it (2026-10-23), it only reduces USB traffic |
   | `fh6-ffb-fix.verbose` | log every driver call, see below | off |

   There is no direction switch: the C5 needs reversed FFB, and FH6's own *Invert Force
   Feedback = On* does that.
6. **Log:** `%TEMP%\fh6-ffb-fix.log`, appended per session. Normally only the patch line,
   the actuator switch, the dedupe state and failures. With the verbose marker: every driver
   call up to 3000 lines per session (failures always), effect updates only every 100th, the
   two polling calls only on change; effect lines show type, download id before/after, gain,
   the type-specific parameters (magnitude, coefficients), and axes/directions before and
   after the repair.

## Build

```powershell
cd dll
cargo build --release   # target\release\xinput1_3.dll
cargo test
```

The toolchain is pinned to `stable-x86_64-pc-windows-gnu` in `rust-toolchain.toml`: it
links with its bundled gcc and needs no Visual Studio. The crate has no dependencies;
the Win32 declarations are hand-written in `src/sys.rs`.

## Install and remove

[`install.ps1`](../install.ps1) copies the DLL into the FH6 folder (next to
`forzahorizon6.exe`) together with the rest of the setup; [`uninstall.ps1`](../uninstall.ps1)
removes it. By hand: copy `target\release\xinput1_3.dll` there, delete it to undo. It is not a
game file, so a Steam file verification leaves it in place.

## Risks and limits

- **It is an in-process hook of the game.** That carries an anti-cheat / terms-of-service
  risk. FH6 ships no anti-cheat files in its folder, which is no guarantee for online play.
  Use at your own risk.
- **Layout assumptions.** Slot 9 and the `DIEFFECT` layout come from the DirectInput
  headers and from reverse engineering `pid.dll` 10.0.26100.8115, where the slot sits at
  `pid.dll+0xc0c8` and points to `+0x4b70`. A different `pid.dll` build is handled by the
  in-image checks above, not by an offset.
- **Updates with `DIEP_DIRECTION` only.** If a parameter update arrives with `cAxes = 2`
  but no axis array, the wrapper forwards it unchanged. Whether `pid.dll` accepts that for an
  effect that was downloaded with one axis is untested.
- **The four ordinal-only exports (100-103)** are undocumented. They are forwarded with
  four register-sized arguments and a full-width return value, which preserves any real
  signature that fits in the x64 argument registers.
