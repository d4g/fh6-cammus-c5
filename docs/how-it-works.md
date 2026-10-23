# How it works

Why each part of this setup is needed. Forza Horizon 6 does not support the Cammus C5, and
two independent layers stand in the way: Windows' generic force-feedback stack, which breaks
for this kind of wheel, and FH6's handling of wheels it does not know. Findings refer to FH6
build 461691 (6.461) on Windows 11.

## Windows' force-feedback stack

### FH6 crashes when the base is the force-feedback device

FH6 asks Windows.Gaming.Input for force feedback. Windows' FFB broker (`ffbroker.dll`) turns
every request into a DirectInput effect over **two axes, X and Z**. DirectInput allows only X
as a force-feedback axis on a device typed as a wheel, so Microsoft's generic HID PID driver
`pid.dll` rejects the Z axis with `E_UNEXPECTED`. Its error path then destroys an effect it
never created and crashes (`pid.dll+0x35c6`, build 10.0.26100.8115).

Most PC racing games use DirectInput directly and create one-axis effects on the wheel's force
axis, so they never take this path. Wheels FH6 supports officially avoid `pid.dll` altogether
(FH6 ships Fanatec's SDK; Logitech and Thrustmaster have their own drivers). Generic HID PID
bases such as Cammus's do not.

**Fix:** the [`xinput1_3.dll` proxy](../dll/README.md). FH6 loads it from its own folder. It
redirects one entry of `pid.dll`'s driver table in memory and rewrites every effect before
`pid.dll` sees it: it drops the axis `pid.dll` would reject and turns the two-axis direction
into the matching sign on X. Strengths and timing are unchanged; no file of Windows or the game
is modified.

### No force at all

Windows.Gaming.Input prepares the device by stopping all effects, switching the actuators off
and resetting it, which per the DirectInput documentation leaves the actuators disabled. It
never switches them back on, so every effect plays muted.

**Fix:** the proxy switches the actuators on itself, at the first moment FH6 actually updates an
effect. Not earlier: while the actuators are off, the C5's firmware centres the wheel by itself,
which is what you want in menus and whenever FH6 does not drive force feedback.

## FH6 and unknown wheels

### Force feedback goes to the pedals

FH6's custom wheel profiles send force feedback to "Device 1", the first controller in
Windows.Gaming.Input's list. With the CP5 connected, that is the pedals. The order cannot be
changed: Windows' "preferred device" setting does not affect it, the pedals hang off the base
and cannot be plugged in later, and Cammus's "2-in-1" mode merges only the rim into the base.

FH6 does find a **default** profile by the base's USB ID, wherever the base sits in the list,
and sends force feedback to that profile's **primary** device.

**Fix:** a default profile whose primary device is the base.

### FH6 ignores added profiles

A new profile file added to FH6's `media\inputmappingprofiles.zip` is never offered, while an
existing entry with only its device ID changed is. Why FH6 tells them apart is not known (it
is not a list of file names or profile IDs in the game).

**Fix:** the profile goes into the existing **Logitech G29** entry and keeps that entry's
profile ID. The name FH6 shows is a key into its string tables, not text, so the label
"CammusC5CP5" is set by replacing the G29's text in the string tables in place (same length,
so nothing else in the file moves).

### Wrong force direction

With the force passed through as FH6 sends it, the wheel overshoots centre and swings from lock
to lock. **Fix:** FH6's own *Invert Force Feedback = On* (Cammus's FH5 guide prescribes the
same).

### Bindings that FH6 shows as unbound

FH6's bundled wheel profiles are older than its keyboard profiles and put some commands in the
wrong context: the telemetry overlay and the convertible top only bind in `RACING_UI`, and the
telemetry page buttons in `TELEMETRY_UI` (the context while the overlay is open). The profile
here follows the keyboard profiles. All commands FH6 knows are listed in
[fh6-input-commands.md](fh6-input-commands.md).

Menu tabs switch with LB/RB, which the paddles carry in menus. The rewind screen offers only
*resume*, *rewind farther* and *cancel*; there is no forward/back scrubbing.

### Where FH6 keeps bindings

Default profiles come only from `media\inputmappingprofiles.zip`. Changes made to them in the
game's menu last until the next start. Saved **custom** profiles are stored in the game's save
container (`C:\XboxGames\GameSave\pgs\u_<id>\…\InputTranslationManager_<user>\Profiles`) and
send force feedback to "Device 1", the pedals. That is why the layout is changed in this
repository, not in the game.

## Telemetry

FH6's Data Out sends the documented 324-byte packet, the same size and layout as FH5. The Cammus
app listens on `127.0.0.1:20779` while FH6 runs and drives the rim's display and LEDs from it;
pointing FH6's Data Out there is all that is needed.

## Upstream

The crash and the missing actuator switch are bugs in Windows (`pid.dll`,
Windows.Gaming.Input); FH6 only triggers them. A community request for official Cammus support
exists on the Forza feedback tracker (FH6S-3094). Neither bug has been reported to Microsoft yet.
