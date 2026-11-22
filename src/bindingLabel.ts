// Plain-language labels for common bindings. Anything unknown falls back to the raw text.

const KEY_NAMES: Record<string, string> = {
  SPACE: "Space", SPC: "Space", RET: "Enter", RETURN: "Enter", ENTER: "Enter", BSPC: "⌫", BACKSPACE: "⌫",
  DEL: "Del", DELETE: "Del", TAB: "Tab", ESC: "Esc", ESCAPE: "Esc",
  LSHFT: "Shift", RSHFT: "Shift", LSHIFT: "Shift", RSHIFT: "Shift", LCTRL: "Ctrl", RCTRL: "Ctrl",
  LALT: "Alt", RALT: "AltGr", LGUI: "Cmd/Win", RGUI: "Cmd/Win",
  LEFT: "←", RIGHT: "→", UP: "↑", DOWN: "↓", HOME: "Home", END: "End", PG_UP: "PgUp", PG_DN: "PgDn",
  SEMI: ";", SEMICOLON: ";", SQT: "'", APOS: "'", COMMA: ",", DOT: ".", FSLH: "/", SLASH: "/", BSLH: "\\",
  MINUS: "-", EQUAL: "=", LBKT: "[", RBKT: "]", LBRC: "{", RBRC: "}", GRAVE: "`", TILDE: "~",
  EXCL: "!", AT: "@", HASH: "#", DLLR: "$", PRCNT: "%", CARET: "^", AMPS: "&", ASTRK: "*",
  LPAR: "(", RPAR: ")", UNDER: "_", PLUS: "+", PIPE: "|", C_VOL_UP: "Vol+", C_VOL_DN: "Vol−",
};

export function keyName(code: string): string {
  if (KEY_NAMES[code]) return KEY_NAMES[code];
  const digit = /^N(UMBER_)?(\d)$/.exec(code);
  if (digit) return digit[2];
  return code;
}

export interface KeyLabel { tap: string; hold?: string; muted?: boolean }

export function bindingLabel(raw: string): KeyLabel {
  const parts = raw.trim().split(/\s+/);
  const [behavior, ...params] = parts;
  switch (behavior) {
    case "&kp": return { tap: keyName(params.join(" ")) };
    case "&trans": return { tap: "▽", muted: true };
    case "&none": return { tap: "", muted: true };
    case "&mo": return { tap: `Layer ${params[0]}`, hold: "hold" };
    case "&to": return { tap: `→ Layer ${params[0]}` };
    case "&tog": return { tap: `Toggle L${params[0]}` };
    case "&sl": return { tap: `Once L${params[0]}` };
    case "&lt": return { tap: keyName(params[1] ?? ""), hold: `Layer ${params[0]}` };
    case "&mt": return { tap: keyName(params[1] ?? ""), hold: keyName(params[0] ?? "") };
    case "&bt":
      if (params[0] === "BT_SEL") return { tap: `BT ${Number(params[1]) + 1}` };
      if (params[0] === "BT_CLR") return { tap: "BT clear" };
      return { tap: params.join(" ") };
    case "&out": return { tap: params[0] === "OUT_USB" ? "USB" : params[0] === "OUT_BLE" ? "Bluetooth" : params.join(" ") };
    case "&caps_word": return { tap: "Caps word" };
    case "&bootloader": return { tap: "Bootloader" };
    case "&sys_reset": return { tap: "Reset" };
    case "&studio_unlock": return { tap: "Studio unlock" };
    default:
      if (!behavior?.startsWith("&")) return { tap: raw };
      return { tap: behavior.slice(1).replace(/_/g, " "), hold: params.length ? params.join(" ") : undefined };
  }
}
