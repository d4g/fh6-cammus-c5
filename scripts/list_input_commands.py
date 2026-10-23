"""List every input command Forza Horizon 6 knows and how its stock profiles bind them.

Sources (read-only):
  - forzahorizon6.exe: every INPUTCMD_* / INPUTCONTEXT_* string, i.e. all commands the game
    knows, including debug and editor commands no profile uses.
  - media\\inputmappingprofiles.zip (the untouched backup copy if present): which command each
    stock profile binds, in which context, and with which input type (Axis / Button / ...).
Output: Markdown at docs/fh6-input-commands.md (relative to this script).

Usage: uv run --no-project python -I list_input_commands.py
"""
import collections
import mmap
import os
import re
import zipfile
from datetime import date
from pathlib import Path

# FH6_DIR: the game folder. FH6_BACKUP_DIR: untouched copies of the game files (see
# install.ps1). Defaults: Steam's standard library and %USERPROFILE%\fh6-backup.
GAME_DIR = Path(os.environ.get("FH6_DIR", r"C:\Program Files (x86)\Steam\steamapps\common\ForzaHorizon6"))
BACKUP_DIR = Path(os.environ.get("FH6_BACKUP_DIR", Path.home() / "fh6-backup"))

EXE = GAME_DIR / "forzahorizon6.exe"
# The stock profiles: prefer the untouched backup, since the game's copy may hold our profile.
PROFILES = next(p for p in (BACKUP_DIR / "inputmappingprofiles.zip",
                            GAME_DIR / "media" / "inputmappingprofiles.zip") if p.exists())
OUT = Path(__file__).resolve().parent.parent / "docs" / "fh6-input-commands.md"

# Profile file name prefix -> device family. RawGameController profiles are the ones wheels
# like the C5 use; WheelMapping profiles are the older Windows.Gaming.Input RacingWheel kind.
FAMILIES = [  # first match wins: "RawGameControllerMappingProfile" also contains "ControllerMappingProfile"
    ("Keyboard", "keyboard"),
    ("RawGameController", "wheel (raw)"),
    ("WheelMapping", "wheel (legacy)"),
    ("ControllerMappingProfile", "gamepad"),
]

# Prefixes of commands that are clearly not for players.
INTERNAL = ("DEBUG_", "TEST_", "DUMP_", "CAMEDIT_", "COE_", "RLE_", "BCD_", "LIVERY_",
            "CYCLE_MODEL_FOCUS_DEBUG", "SWITCHCAMERA_DEBUG", "TOGGLE_FREECAM_DEBUG",
            "TOGGLE_FREEFOCUSCAM_DEBUG", "TOGGLE_VISIBILITY_DEBUG")


def family(name):
    for frag, fam in FAMILIES:
        if frag in name:
            return fam
    return "other"


def main():
    with open(EXE, "rb") as f, mmap.mmap(f.fileno(), 0, access=mmap.ACCESS_READ) as m:
        exe_cmds = sorted(set(x.decode() for x in re.findall(rb"INPUTCMD_[A-Z0-9_]+", m)))
        exe_ctx = sorted(set(x.decode() for x in re.findall(rb"INPUTCONTEXT_[A-Z0-9_]+", m)))

    # context -> command -> {"profiles": n, "families": set, "types": set}
    used = collections.defaultdict(lambda: collections.defaultdict(lambda: {"profiles": 0, "families": set(), "types": set()}))
    z = zipfile.ZipFile(PROFILES)
    for n in z.namelist():
        d = z.read(n).decode("utf-8", "replace")
        fam = family(n)
        for ctx, body in re.findall(r'Context="(\w+)"[^>]*>(.*?)</Context>', d, re.S):
            seen = set()
            for tag in re.finditer(r'<Value [^>]*Key="(INPUTCMD_\w+)"[^>]*?(/?)>', body):
                key = tag.group(1)
                t = re.search(r'InputType="(\w+)"', tag.group(0))
                e = used[ctx][key]
                if key not in seen:
                    e["profiles"] += 1
                    seen.add(key)
                e["families"].add(fam)
                if t:
                    e["types"].add(t.group(1))
                elif not tag.group(2):  # <Value ...> with InputCmdLow/High children: axis from two buttons
                    e["types"].add("Low/High pair")

    bound = {k for c in used.values() for k in c}
    lines = [
        "# Forza Horizon 6 — input commands",
        "",
        f"Generated {date.today()} by `scripts/list_input_commands.py` from `forzahorizon6.exe` and the stock",
        "`media\\inputmappingprofiles.zip` (untouched copy). Regenerate after a game update.",
        "",
        f"The exe contains **{len(exe_cmds)} commands** in **{len(exe_ctx)} contexts**; the stock profiles bind",
        f"**{len(bound)}** of them in **{len(used)}** contexts. A context is the game situation a binding",
        "applies in (driving, menus, photo mode, ...); a profile lists its bindings per context.",
        "",
        "Columns: *Profiles* = how many of the stock profiles bind the command in that context;",
        "*Devices* = which kinds of profile do (keyboard, gamepad, raw wheel = the kind the C5 uses,",
        "legacy wheel); *Input* = input types used (`Axis`, `Button`, `Low/High pair` = an axis made",
        "from two buttons, e.g. map scrolling).",
        "",
        "## Commands bound by the stock profiles, per context",
        "",
    ]
    for ctx in sorted(used):
        lines += [f"### `{ctx}`", "", "| Command | Profiles | Devices | Input |", "|---|---|---|---|"]
        for key in sorted(used[ctx]):
            e = used[ctx][key]
            lines.append(f"| `{key}` | {e['profiles']} | {', '.join(sorted(e['families']))} | {', '.join(sorted(e['types'])) or '—'} |")
        lines.append("")

    unbound = [c for c in exe_cmds if c not in bound]
    player = [c for c in unbound if not c.removeprefix("INPUTCMD_").startswith(INTERNAL)]
    internal = [c for c in unbound if c not in player]
    lines += [
        "## Known to the exe, bound by no stock profile",
        "",
        "Whether the game accepts these from a profile is **untested**. The first list looks",
        "player-facing; the second is debug, test and editor tooling.",
        "",
        "Possibly player-facing:",
        "",
        ", ".join(f"`{c}`" for c in player),
        "",
        "Internal (debug, test, editors):",
        "",
        ", ".join(f"`{c}`" for c in internal),
        "",
        "## Contexts known to the exe",
        "",
        ", ".join(f"`{c}`" + ("" if c in used else " (no stock bindings)") for c in exe_ctx),
        "",
    ]
    OUT.write_text("\n".join(lines), encoding="utf-8")
    print(f"wrote {OUT}: {len(exe_cmds)} commands, {len(bound)} bound, {len(player)} unbound player-facing, {len(internal)} internal")


if __name__ == "__main__":
    main()
