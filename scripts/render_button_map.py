"""Draw the C5 rim's real button numbers and their FH6 functions onto the rim drawing.

Inputs: the numbered rim drawing (PNG) and capture_buttons.csv from capture_buttons.ps1.
The functions come from the repo profile profile/DefaultRawGameControllerMappingProfileCammusC5.xml
(bindings on the rim 0x34161021). Output: a PNG with the drawing on the left (each label
replaced by a badge with the real button index) and a legend on the right.

English names are FH6's own (Controls > Wheel), so the image matches the game's menu.

Usage: uv run --no-project --with pillow python -I render_button_map.py <drawing.png> <out.png>
           [--profile profile.xml] [--lang en|de] [--title TEXT]
"""
import argparse
import csv
import re
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

HERE = Path(__file__).resolve().parent
PROFILE = HERE.parent / "profile" / "DefaultRawGameControllerMappingProfileCammusC5.xml"
CSV = HERE / "capture_buttons.csv"
RIM = "0x34161021"

# Label positions in the 1024 px drawing (centre of each printed number).
POS = {
    "1": (82, 563), "2": (925, 563), "3": (330, 428), "7": (300, 477),
    "4": (310, 551), "5a": (345, 516), "5b": (381, 551), "6": (345, 588),
    "8": (466, 530), "11": (543, 530), "13": (466, 588), "14a": (543, 588),
    "9": (295, 625), "10a": (318, 678),
    "17L": (378, 676), "17R": (428, 676), "18L": (480, 728), "18R": (530, 728),
    "19L": (581, 676), "19R": (631, 676),
    "20": (683, 428), "10b": (709, 479),
    "21": (663, 515), "22": (628, 551), "23": (699, 551), "14b": (663, 588),
    "15": (714, 625), "16": (690, 678),
}
SKIP = {"L#", "R#", "17", "18", "19"}  # centre squares and knob "presses" do not exist

# Command -> (English as in FH6's menu, German).
NAMES = {
    "INPUTCMD_SHIFTUP": ("Shift Up", "Hochschalten"),
    "INPUTCMD_SHIFTDOWN": ("Shift Down", "Runterschalten"),
    "INPUTCMD_HANDBRAKE": ("E-Brake", "Handbremse"),
    "INPUTCMD_SWITCH_CAMERA": ("Switch Camera", "Kamera"),
    "INPUTCMD_MULLIGAN": ("Rewind", "Zurückspulen"),
    "INPUTCMD_ACTIVATE": ("Activate", "Activate"),
    "INPUTCMD_PAUSE_GAME": ("Pause", "Pause"),
    "INPUTCMD_RADIO_RIGHT": ("Radio Next", "Radio weiter"),
    "INPUTCMD_RADIO_LEFT": ("Radio Previous", "Radio zurück"),
    "INPUTCMD_ANNA_ACTIVATE": ("Activate ANNA", "ANNA"),
    "INPUTCMD_QUICKCHAT_ACTIVATE": ("Forza LINK", "Forza LINK"),
    "INPUTCMD_ANNA_ITEM_1": ("ANNA option 1", "ANNA 1"),
    "INPUTCMD_ANNA_ITEM_2": ("ANNA option 2", "ANNA 2"),
    "INPUTCMD_ANNA_ITEM_3": ("ANNA option 3", "ANNA 3"),
    "INPUTCMD_ANNA_ITEM_4": ("ANNA option 4", "ANNA 4"),
    "INPUTCMD_HORN": ("Horn", "Hupe"),
    "INPUTCMD_LOOK_BACK": ("Look Back", "Blick zurück"),
    "INPUTCMD_LOOK_LEFT": ("Look Left", "Blick links"),
    "INPUTCMD_LOOK_RIGHT": ("Look Right", "Blick rechts"),
    "INPUTCMD_TELEMETRY_TOGGLE": ("Telemetry", "Telemetrie an/aus"),
    "INPUTCMD_TELEMETRY_PREV": ("Telemetry Previous", "Telemetrie zurück"),
    "INPUTCMD_TELEMETRY_NEXT": ("Telemetry Next", "Telemetrie vor"),
    "INPUTCMD_PHOTO_MODE_TOGGLE": ("Photo Mode", "Fotomodus"),
    "INPUTCMD_AUTODRIVE_CINEMATIC_CAMERA": ("AutoDrive Cinematic Camera", "Autodrive-Kinokamera"),
    "INPUTCMD_OPEN_MAP": ("Map", "Karte"),
    "INPUTCMD_CONVERTIBLE_TRIGGER": ("Toggle Convertible", "Verdeck"),
    "INPUTCMD_UI_REPLAY_SPEED_LEFT": ("Replay slower", "Replay langsamer"),
    "INPUTCMD_UI_REPLAY_SPEED_RIGHT": ("Replay faster", "Replay schneller"),
    "INPUTCMD_HIDESEEK_PING_OR_CHASEBREAK": ("Ping / ChaseBreaker (Hide & Seek)", "Ping / ChaseBreaker (Hide & Seek)"),
}
# Menu commands INPUTCMD_UI_<name>_PRESS/RELEASE/REPEAT -> (English, German).
UI = {
    "OK": ("OK", "OK"), "CANCEL": ("Back", "zurück"), "X": ("X", "X"), "Y": ("Y", "Y"),
    "START": ("Start (Menu button)", "Start (Menu-Taste)"), "BACK": ("View button", "View-Taste"),
    "LBUMPER": ("LB / previous tab", "LB / Reiter zurück"), "RBUMPER": ("RB / next tab", "RB / Reiter vor"),
    "LTRIGGER": ("LT", "LT"), "RTRIGGER": ("RT", "RT"),
    "UP": ("up", "hoch"), "DOWN": ("down", "runter"), "LEFT": ("left", "links"), "RIGHT": ("right", "rechts"),
}
TEXT = {
    "title": ("Cammus C5 rim: button number -> function in Forza Horizon 6",
              "Cammus C5 Lenkrad: Tastennummer -> Funktion in Forza Horizon 6"),
    "button": ("Button", "Taste"),
    "function": ("Function (driving | menu)", "Funktion (Fahren | Menü)"),
    "menu": ("Menu", "Menü"),
    "free": ("free", "frei"),
    "footer": ("Blue = bound, grey = free. Knobs 17/18/19: one button per turn direction.",
               "Blau = belegt, grau = frei. Regler 17/18/19: eine Taste pro Drehrichtung."),
}
# Duplicates of other menu bindings that would only clutter the legend.
HIDDEN = re.compile(r"INPUTCMD_UI_(DPAD_|MAP_|BRICKCHALLENGES_)|INPUTCMD_(PREV|NEXT)_CATEGORY$")


def functions_by_button(profile, lang):
    """Rim button index -> (driving functions, menu functions), from the profile's contexts:
    INPUTCONTEXT_UI is the menu, every other context counts as driving."""
    i = 0 if lang == "en" else 1
    xml = Path(profile).read_text(encoding="utf-8-sig")
    drive, menu = {}, {}
    for ctx, body in re.findall(r'Context="(\w+)">(.*?)</Context>', xml, re.S):
        target = menu if ctx == "INPUTCONTEXT_UI" else drive
        for key, idx in re.findall(r'Key="(\w+)" VidPid="' + RIM + r'" InputType="Button" Index="(\d+)"', body):
            if HIDDEN.search(key):
                continue
            m = re.match(r"INPUTCMD_UI_(\w+?)_(PRESS|RELEASE|REPEAT|WHILEDOWN)$", key)
            pair = UI.get(m.group(1)) if m else NAMES.get(key)
            name = pair[i] if pair else key.replace("INPUTCMD_", "").title()
            if name not in target.setdefault(int(idx), []):
                target[int(idx)].append(name)
    return {k: (drive.get(k, []), menu.get(k, [])) for k in set(drive) | set(menu)}


def describe(f, menu_word):
    drive, menu = f
    parts = [", ".join(drive)] if drive else []
    if menu:
        parts.append(f"{menu_word}: " + ", ".join(menu))
    return "  |  ".join(parts)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("drawing")
    ap.add_argument("out")
    ap.add_argument("--profile", default=PROFILE)
    ap.add_argument("--lang", choices=("en", "de"), default="en")
    ap.add_argument("--title")
    args = ap.parse_args()
    t = {k: v[0 if args.lang == "en" else 1] for k, v in TEXT.items()}

    rows = []
    with open(CSV, encoding="utf-8-sig") as f:
        for r in csv.DictReader(f):
            m = re.search(r"button (\d+)", r["Reported"])
            if r["Label"] in SKIP or not m:
                continue
            rows.append((r["Label"], int(m.group(1)), r["Control"]))
    funcs = functions_by_button(args.profile, args.lang)

    img = Image.open(args.drawing).convert("RGB")
    w, h = img.size
    legend_w = 900
    canvas = Image.new("RGB", (w + legend_w, max(h, 80 + 34 * (len(rows) + 2))), "white")
    canvas.paste(img, (0, 0))
    d = ImageDraw.Draw(canvas)
    font = ImageFont.truetype("arialbd.ttf", 22)
    small = ImageFont.truetype("arial.ttf", 20)
    head = ImageFont.truetype("arialbd.ttf", 24)

    used, free = (30, 110, 200), (150, 150, 150)
    for label, idx, _ in rows:
        x, y = POS[label]
        col = used if funcs.get(idx) else free
        r = 19
        d.ellipse((x - r, y - r, x + r, y + r), fill=col, outline="white", width=2)
        s = str(idx)
        d.text((x - d.textlength(s, font=font) / 2, y - 13), s, fill="white", font=font)

    x0, y = w + 20, 20
    d.text((x0, y), args.title or t["title"], fill="black", font=head)
    y += 44
    d.text((x0, y), t["button"], fill="black", font=font)
    d.text((x0 + 80, y), t["function"], fill="black", font=font)
    y += 34
    for _, idx, _ in sorted(rows, key=lambda r: r[1]):
        f = funcs.get(idx)
        d.text((x0, y), str(idx), fill=used if f else free, font=font)
        d.text((x0 + 80, y), describe(f, t["menu"]) if f else t["free"], fill="black" if f else free, font=small)
        y += 34
    y += 10
    d.text((x0, y), t["footer"], fill="black", font=small)
    canvas.save(args.out)
    print("saved", args.out)


if __name__ == "__main__":
    main()
