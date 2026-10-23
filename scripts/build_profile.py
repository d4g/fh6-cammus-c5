"""Build the Cammus C5 + CP5 mapping profile for FH6 from the binding table below.

The table is the source of truth for the rim layout; the XML is generated in the style of
FH6's stock RawGameController profiles (CRLF, no BOM, one <Value> per binding). Menu
commands come as PRESS/RELEASE/REPEAT triples like in the stock profiles.

Layout rationale (2026-10-23):
  - left D-pad 10-13 = Xbox D-pad: menu navigation; while driving FH6's own D-pad layout
    (photo mode, quick chat, ANNA, radio) taken from its Logitech G920 profile;
  - right D-pad 18-21 = face buttons in Xbox/PlayStation position (down = A/Cross = OK,
    right = B/Circle = back, left = X/Square, up = Y/Triangle);
  - rotary knobs are +/- pairs: knob 17 (24/25) radio, knob 18 (38/39) telemetry pages;
  - paddles switch tabs (LB/RB) in menus and shift while driving; LT/RT sit on buttons 0/4.

Usage: uv run --no-project python -I build_profile.py [out.xml]
       (default out: ../profile/DefaultRawGameControllerMappingProfileCammusC5.xml)
"""
import sys
from pathlib import Path

BASE, RIM, PEDALS = "0x34160301", "0x34161021", "0x34161018"

# context -> list of (command, rim button). "UI_*" menu commands are expanded to
# PRESS/RELEASE/REPEAT, as the stock profiles do.
RIM_BINDINGS = {
    "INPUTCONTEXT_RACING": [
        ("SHIFTUP", 9), ("SHIFTDOWN", 8),
        ("HANDBRAKE", 5),
        ("HORN", 20),
        ("ACTIVATE", 19),
        ("MULLIGAN", 7),
        ("LOOK_BACK", 21), ("LOOK_LEFT", 0), ("LOOK_RIGHT", 4),
        ("SWITCH_CAMERA", 18),
        ("PAUSE_GAME", 6),
        ("AUTODRIVE_CINEMATIC_CAMERA", 16),
        ("OPEN_MAP", 14),
    ],
    # The contexts below follow FH6's own keyboard profiles (format version 3), which are newer
    # than the stock wheel profiles (version 1): those put telemetry in RACING, where FH6 6.461
    # ignores it. Verified in game 2026-10-23: CONVERTIBLE_TRIGGER and TELEMETRY_TOGGLE only
    # bind from RACING_UI; TELEMETRY_PREV/NEXT live in TELEMETRY_UI.
    "INPUTCONTEXT_RACING_UI": [
        ("PHOTO_MODE_TOGGLE", 10), ("QUICKCHAT_ACTIVATE", 11), ("ANNA_ACTIVATE", 12),
        ("RADIO_RIGHT", 13), ("RADIO_RIGHT", 25), ("RADIO_LEFT", 24),
        ("CONVERTIBLE_TRIGGER", 15),
        ("TELEMETRY_TOGGLE", 1),
    ],
    "INPUTCONTEXT_TELEMETRY_UI": [
        ("TELEMETRY_TOGGLE", 1), ("TELEMETRY_PREV", 38), ("TELEMETRY_NEXT", 39),   # verified 2026-10-23
    ],
    "INPUTCONTEXT_RACING_CAMERA_ONLY": [
        ("ACTIVATE", 19), ("ANNA_ACTIVATE", 12), ("HORN", 20), ("PAUSE_GAME", 6),
        ("SWITCH_CAMERA", 18),
    ],
    "INPUTCONTEXT_COPTER": [
        ("PHOTO_MODE_TOGGLE", 10), ("QUICKCHAT_ACTIVATE", 11), ("ANNA_ACTIVATE", 12),
        ("RADIO_RIGHT", 13), ("RADIO_RIGHT", 25), ("RADIO_LEFT", 24),
    ],
    "INPUTCONTEXT_HIDE_SEEK": [
        ("HIDESEEK_PING_OR_CHASEBREAK", 2),   # Hide & Seek only; name from FH6's saved profiles
    ],
    "INPUTCONTEXT_ANNA": [
        ("ANNA_ITEM_1", 10), ("ANNA_ITEM_2", 11), ("ANNA_ITEM_3", 13), ("ANNA_ITEM_4", 12),
    ],
    "INPUTCONTEXT_UI": [
        ("UI_OK", 20), ("UI_CANCEL", 21), ("UI_X", 19), ("UI_Y", 18),
        ("UI_START", 6), ("UI_BACK", 3),   # Back moved off 7: 7 is rewind, also on the rewind screen
        # Paddles switch menu tabs (LB/RB + category), as in FH6's G920 profile. LT/RT on the
        # paddles did not switch tabs; RT closed the menu instead (tested 2026-10-23).
        ("UI_LBUMPER", 8), ("UI_RBUMPER", 9), ("PREV_CATEGORY", 8), ("NEXT_CATEGORY", 9),
        ("UI_LTRIGGER", 0), ("UI_RTRIGGER", 4),
        ("UI_UP", 10), ("UI_LEFT", 11), ("UI_DOWN", 12), ("UI_RIGHT", 13),
        ("UI_DPAD_UP", 10), ("UI_DPAD_LEFT", 11), ("UI_DPAD_DOWN", 12), ("UI_DPAD_RIGHT", 13),
        ("MULLIGAN", 7),
        # Replay playback speed on knob 19 (48 = slower, 49 = faster). The rewind screen has no
        # scrubbing, only resume (OK) / rewind farther (MULLIGAN) / cancel. The stock profiles
        # bind these analog (pedals, keys); whether button pulses work: untested.
        ("UI_REPLAY_SPEED_LEFT", 48), ("UI_REPLAY_SPEED_RIGHT", 49),
    ],
}
# Stock profiles bind only *_PRESS for these.
PRESS_ONLY = {"UI_DPAD_UP", "UI_DPAD_LEFT", "UI_DPAD_DOWN", "UI_DPAD_RIGHT"}
TRIPLE = {"UI_OK", "UI_CANCEL", "UI_X", "UI_Y", "UI_START", "UI_BACK", "UI_LBUMPER", "UI_RBUMPER",
          "UI_LTRIGGER", "UI_RTRIGGER", "UI_UP", "UI_LEFT", "UI_DOWN", "UI_RIGHT"}


def value(key, vidpid, input_type, index, extra=""):
    return f'    <Value Version="1" Key="INPUTCMD_{key}" VidPid="{vidpid}" InputType="{input_type}" Index="{index}"{extra} />'


def build():
    out = [
        "<Profiles>",
        '<RawGameControllerInputMappingProfile Version="1" Id="34160301-F317-4F97-8D1E-87E6C5F696AF" '
        'UserFacingName="IDS_DefaultWheelProfile_Name" IsDefaultProfile="1" PrimaryDeviceVidPid="0x34160301" '
        'FFBDeviceVidPid="0x34160301" FFBMotorIndex="0">',
        "<!-- Cammus C5 base 0x34160301 (steering, FFB), C5 rim 0x34161021 (buttons), CP5 pedals 0x34161018.",
        "     Generated by scripts/build_profile.py; edit the table there, not this file.",
        "     Rim button numbers measured with scripts/capture_buttons.ps1. See README.md and docs/how-it-works.md. -->",
        "",
    ]
    for ctx, bindings in RIM_BINDINGS.items():
        out.append(f'  <Context Version="1" Context="{ctx}">')
        if ctx == "INPUTCONTEXT_RACING":
            out.append(value("STEERING", BASE, "Axis", 0,
                             ' InvertAxis="false" DeadzonesAroundCenter="true" InnerDeadzone="0.00" OuterDeadzone="1.00"'))
            for key, idx in (("GAS", 0), ("BRAKE", 1), ("CLUTCH", 2)):
                out.append(value(key, PEDALS, "Axis", idx, ' InvertAxis="false" InnerDeadzone="0.05" OuterDeadzone="0.95"'))
            out.append('    <Value Version="1" Key="INPUTCMD_GEAR_REVERSE" />')
        for key, idx in bindings:
            if key in TRIPLE:
                out += [value(f"{key}_{s}", RIM, "Button", idx) for s in ("PRESS", "RELEASE", "REPEAT")]
            elif key in PRESS_ONLY:
                out.append(value(f"{key}_PRESS", RIM, "Button", idx))
            else:
                out.append(value(key, RIM, "Button", idx))
        if ctx == "INPUTCONTEXT_UI":
            # Map scrolling as low/high button pairs on the left D-pad, as before.
            for key, low, high in (("UI_MAP_MOVE_LEFTRIGHT", 11, 13), ("UI_MAP_MOVE_UPDOWN", 12, 10)):
                out += [f'    <Value Version="1" Key="INPUTCMD_{key}">',
                        f'      <InputCmdLow VidPid="{RIM}" InputType="Button" Index="{low}" />',
                        f'      <InputCmdHigh VidPid="{RIM}" InputType="Button" Index="{high}" />',
                        "    </Value>"]
        out += ["  </Context>", ""]
    out += ["</RawGameControllerInputMappingProfile>", "</Profiles>", ""]
    return "\r\n".join(out).encode("utf-8")


if __name__ == "__main__":
    dst = Path(sys.argv[1]) if len(sys.argv) > 1 else \
        Path(__file__).resolve().parent.parent / "profile" / "DefaultRawGameControllerMappingProfileCammusC5.xml"
    dst.write_bytes(build())
    print("wrote", dst)
