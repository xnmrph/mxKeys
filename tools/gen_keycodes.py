#!/usr/bin/env python3
"""Generate crates/zmk-config/data/keycodes.json from ZMK's dt-bindings/zmk/keys.h.

keys.h (MIT, (c) The ZMK Contributors) is organised in blocks:

    /* Keyboard a and A */
    #define A (ZMK_HID_USAGE(HID_USAGE_KEY, HID_USAGE_KEY_KEYBOARD_A))
    #define ALIAS (A)

Each block becomes one entry: all names (canonical first), the description, the HID
page, and a category + short label for the UI.

Usage: python3 tools/gen_keycodes.py <path/to/keys.h> <zmk-commit> > crates/zmk-config/data/keycodes.json
"""
import json
import re
import sys

LABELS = {
    "ENTER": "Enter", "RETURN": "Enter", "ESCAPE": "Esc", "BACKSPACE": "⌫", "TAB": "Tab", "SPACE": "Space",
    "DELETE": "Del", "INSERT": "Ins", "HOME": "Home", "END": "End", "PAGE_UP": "PgUp", "PAGE_DOWN": "PgDn",
    "RIGHT_ARROW": "→", "LEFT_ARROW": "←", "DOWN_ARROW": "↓", "UP_ARROW": "↑",
    "LEFT_SHIFT": "Shift", "RIGHT_SHIFT": "Shift", "LEFT_CONTROL": "Ctrl", "RIGHT_CONTROL": "Ctrl",
    "LEFT_ALT": "Alt", "RIGHT_ALT": "AltGr", "LEFT_GUI": "Cmd/Win", "RIGHT_GUI": "Cmd/Win",
    "CAPSLOCK": "Caps Lock", "PRINTSCREEN": "PrtSc", "SCROLLLOCK": "ScrLk", "PAUSE_BREAK": "Pause",
    "C_VOLUME_UP": "Vol+", "C_VOLUME_DOWN": "Vol−", "C_MUTE": "Mute", "C_PLAY_PAUSE": "Play/Pause",
    "C_NEXT": "Next", "C_PREVIOUS": "Prev", "C_BRIGHTNESS_INC": "Bright+", "C_BRIGHTNESS_DEC": "Bright−",
}

def category(page, canonical, desc, shifted=False):
    d = desc.lower()
    if shifted:
        return "symbols"
    if page == "consumer":
        return "media"
    if page == "gd":
        return "system"
    if re.fullmatch(r"[A-Z]", canonical):
        return "letters"
    if re.fullmatch(r"NUMBER_\d", canonical):
        return "numbers"
    if re.fullmatch(r"F\d+", canonical):
        return "function"
    if canonical.startswith("KP_") or "keypad" in d:
        return "keypad"
    if any(m in canonical for m in ("SHIFT", "CONTROL", "_ALT", "_GUI")):
        return "modifiers"
    if any(k in canonical for k in ("ARROW", "HOME", "END", "PAGE_", "INSERT", "DELETE")):
        return "navigation"
    if canonical in ("ENTER", "RETURN", "ESCAPE", "BACKSPACE", "TAB", "SPACE", "CAPSLOCK", "PRINTSCREEN", "SCROLLLOCK", "PAUSE_BREAK"):
        return "editing"
    if "international" in d or "lang" in d or canonical.startswith(("INT", "LANG")):
        return "international"
    if re.search(r"keyboard \S+ and \S+", d) or "non-us" in d:
        return "symbols"
    return "other"

def label(canonical, names, desc):
    if canonical in LABELS:
        return LABELS[canonical]
    m = re.fullmatch(r"NUMBER_(\d)", canonical)
    if m:
        return m.group(1)
    if re.fullmatch(r"[A-Z]|F\d+", canonical):
        return canonical
    # "Keyboard ; and :" → ";"   "Keyboard { (Left Brace)" → "{"
    m = re.match(r"Keyboard (\S+) and (\S+)", desc)
    if m and len(m.group(1)) <= 2:
        return m.group(1)
    m = re.match(r"Keyboard (\S{1,2}) \(", desc)
    if m:
        return m.group(1)
    # "Keypad 0" → "Num 0", "Keypad - (Minus)" → "Num -"
    m = re.match(r"Keypad (\S{1,2})(?: |$)", desc)
    if m:
        return "Num " + m.group(1)
    short = min(names, key=len)
    return short

def main(path, commit):
    text = open(path, encoding="utf-8").read().replace("\\\n", " ")
    entries = []
    desc = None
    current = None
    for raw in text.splitlines():
        deprecated = "DEPRECATED" in raw
        line = re.sub(r"\s*//.*$", "", raw).strip()
        line = re.sub(r"\s+", " ", line)
        c = re.fullmatch(r"/\*\s*(.*?)\s*\*/", line)
        if c:
            desc = c.group(1)
            current = None
            continue
        d = re.fullmatch(r"#define\s+(\w+)\s+\((.*)\)", line)
        if not d:
            continue
        name, body = d.group(1), d.group(2).strip()
        shifted = False
        s_ = re.fullmatch(r"LS\((.*)\)", body)
        if s_:
            body, shifted = s_.group(1).strip(), True
        u = re.fullmatch(r"ZMK_HID_USAGE\((HID_USAGE_\w+?),\s*(\w+)\)", body)
        if u:
            page = {"HID_USAGE_KEY": "key", "HID_USAGE_CONSUMER": "consumer", "HID_USAGE_GD": "gd"}.get(u.group(1), u.group(1))
            current = {"names": [name], "description": desc or name, "page": page, "usage": u.group(2), "shifted": shifted, "deprecated": []}
            entries.append(current)
        elif current is not None and re.fullmatch(r"\w+", body):
            if body in current["names"]:
                (current["deprecated"] if deprecated else current["names"]).append(name)
    for e in entries:
        e["category"] = category(e["page"], e["names"][0], e["description"], e["shifted"])
        e["label"] = label(e["names"][0], e["names"], e["description"])
    json.dump({"source": f"zmkfirmware/zmk app/include/dt-bindings/zmk/keys.h @ {commit} (MIT)", "keys": entries},
              sys.stdout, indent=1, ensure_ascii=False)
    sys.stdout.write("\n")

if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
