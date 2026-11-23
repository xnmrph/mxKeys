//! The settings page: a catalog of well-known ZMK/Zephyr Kconfig options with plain
//! names, units, ranges and defaults, read from and written to a `.conf` file via the
//! lossless [`KconfigFile`] model.
//!
//! Defaults and ranges were checked against ZMK `main` (4de3133, Zephyr 4.1) and a real
//! build's `.config` for the Corne + nice!nano v2 + nice!view profile. Defaults that
//! depend on the hardware (the screen) are the ones for that profile.
//!
//! Semantics:
//! - Setting a value writes `CONFIG_X=value` (replacing, uncommenting or appending).
//! - "Use the default" comments out every line that sets the option, so ZMK's default
//!   applies again and the user's old value is still visible in the file.
//! - Choices (one-of-many Kconfig `choice`s like Bluetooth power) write the chosen
//!   option `=y` and comment out the other options that are set.
//! - Some switches need companion options (`also`), e.g. RGB underglow needs the LED
//!   strip driver; they are switched on together.

use serde::{Deserialize, Serialize};

use crate::dts::Patch;
use crate::kconfig::{KconfigFile, KconfigValue, SettingState};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Group {
    Power,
    Bluetooth,
    Screen,
    Lights,
    Typing,
    LiveEditing,
}

impl Group {
    pub const ALL: [Group; 6] = [
        Group::Power,
        Group::Bluetooth,
        Group::Screen,
        Group::Lights,
        Group::Typing,
        Group::LiveEditing,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Group::Power => "Battery and sleep",
            Group::Bluetooth => "Bluetooth",
            Group::Screen => "Screen",
            Group::Lights => "Lights",
            Group::Typing => "Typing",
            Group::LiveEditing => "Live editing",
        }
    }
}

/// How a number is shown. Values are always stored as Kconfig writes them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Unit {
    /// Stored in milliseconds, shown in seconds.
    Seconds,
    /// Stored in milliseconds, shown in minutes.
    Minutes,
    Milliseconds,
    Percent,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChoiceOption {
    /// The Kconfig option for this choice (`None` for an int-valued pick).
    pub option: Option<&'static str>,
    /// Value written for an int-valued pick.
    pub value: Option<i64>,
    pub label: &'static str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Kind {
    Toggle {
        option: &'static str,
        default: bool,
        /// Switched on together with this one.
        also: &'static [&'static str],
    },
    Number {
        option: &'static str,
        default: i64,
        min: i64,
        max: i64,
        unit: Unit,
    },
    Text {
        option: &'static str,
        default: &'static str,
        max_bytes: usize,
    },
    /// A Kconfig `choice`: one bool option per entry.
    Choice {
        options: Vec<ChoiceOption>,
        default: usize,
    },
    /// An int option with named values.
    Pick {
        option: &'static str,
        options: Vec<ChoiceOption>,
        default: usize,
    },
}

#[derive(Debug, Clone, Serialize)]
pub struct SettingDef {
    pub id: &'static str,
    pub group: Group,
    pub label: &'static str,
    pub help: &'static str,
    pub kind: Kind,
    /// Only matters while this toggle (by id) is on.
    pub requires: Option<&'static str>,
}

impl SettingDef {
    /// Every Kconfig option this setting reads or writes (for the Advanced view).
    pub fn options(&self) -> Vec<&'static str> {
        match &self.kind {
            Kind::Toggle { option, also, .. } => {
                std::iter::once(*option).chain(also.iter().copied()).collect()
            }
            Kind::Number { option, .. } | Kind::Text { option, .. } | Kind::Pick { option, .. } => {
                vec![option]
            }
            Kind::Choice { options, .. } => options.iter().filter_map(|o| o.option).collect(),
        }
    }
}

fn toggle(option: &'static str, default: bool) -> Kind {
    Kind::Toggle {
        option,
        default,
        also: &[],
    }
}

fn opt(option: &'static str, label: &'static str) -> ChoiceOption {
    ChoiceOption {
        option: Some(option),
        value: None,
        label,
    }
}

fn val(value: i64, label: &'static str) -> ChoiceOption {
    ChoiceOption {
        option: None,
        value: Some(value),
        label,
    }
}

/// nRF52840 radio power levels Zephyr offers (`BT_CTLR_TX_PWR_*`), strongest first.
const TX_POWER: [(&str, &str); 13] = [
    ("CONFIG_BT_CTLR_TX_PWR_PLUS_8", "+8 dBm — strongest, most battery"),
    ("CONFIG_BT_CTLR_TX_PWR_PLUS_7", "+7 dBm"),
    ("CONFIG_BT_CTLR_TX_PWR_PLUS_6", "+6 dBm"),
    ("CONFIG_BT_CTLR_TX_PWR_PLUS_5", "+5 dBm"),
    ("CONFIG_BT_CTLR_TX_PWR_PLUS_4", "+4 dBm"),
    ("CONFIG_BT_CTLR_TX_PWR_PLUS_3", "+3 dBm"),
    ("CONFIG_BT_CTLR_TX_PWR_PLUS_2", "+2 dBm"),
    ("CONFIG_BT_CTLR_TX_PWR_0", "0 dBm — normal"),
    ("CONFIG_BT_CTLR_TX_PWR_MINUS_4", "−4 dBm"),
    ("CONFIG_BT_CTLR_TX_PWR_MINUS_8", "−8 dBm"),
    ("CONFIG_BT_CTLR_TX_PWR_MINUS_12", "−12 dBm"),
    ("CONFIG_BT_CTLR_TX_PWR_MINUS_16", "−16 dBm"),
    ("CONFIG_BT_CTLR_TX_PWR_MINUS_20", "−20 dBm — weakest"),
];

/// The settings shown for the Corne + nice!nano v2 + nice!view profile.
pub fn catalog() -> Vec<SettingDef> {
    use Group::*;
    vec![
        SettingDef {
            id: "idle_timeout",
            group: Power,
            label: "Idle after",
            help: "After this long without a key press the keyboard counts as idle: lights turn off and, if you choose, the screen too.",
            kind: Kind::Number { option: "CONFIG_ZMK_IDLE_TIMEOUT", default: 30_000, min: 1_000, max: 86_400_000, unit: Unit::Seconds },
            requires: None,
        },
        SettingDef {
            id: "sleep",
            group: Power,
            label: "Deep sleep when unused",
            help: "Saves a lot of battery when you walk away. Any key press wakes the keyboard; it reconnects within a few seconds (the first key press isn't typed).",
            kind: toggle("CONFIG_ZMK_SLEEP", false),
            requires: None,
        },
        SettingDef {
            id: "sleep_timeout",
            group: Power,
            label: "Deep sleep after",
            help: "How long without a key press before deep sleep.",
            kind: Kind::Number { option: "CONFIG_ZMK_IDLE_SLEEP_TIMEOUT", default: 900_000, min: 60_000, max: 86_400_000, unit: Unit::Minutes },
            requires: Some("sleep"),
        },
        SettingDef {
            id: "battery_both_halves",
            group: Power,
            label: "Show both halves' battery on the computer",
            help: "Normally the computer only sees the left half's battery. This also reports the right half (as a second battery; not every system shows it).",
            kind: Kind::Toggle {
                option: "CONFIG_ZMK_SPLIT_BLE_CENTRAL_BATTERY_LEVEL_FETCHING",
                default: false,
                also: &["CONFIG_ZMK_SPLIT_BLE_CENTRAL_BATTERY_LEVEL_PROXY"],
            },
            requires: None,
        },
        SettingDef {
            id: "keyboard_name",
            group: Bluetooth,
            label: "Keyboard name",
            help: "The name your computer and phone show when pairing (16 characters at most). After changing it you may need to forget and pair the keyboard again.",
            kind: Kind::Text { option: "CONFIG_ZMK_KEYBOARD_NAME", default: "Corne", max_bytes: 16 },
            requires: None,
        },
        SettingDef {
            id: "tx_power",
            group: Bluetooth,
            label: "Signal strength",
            help: "A stronger signal helps when the computer is far away or behind something, and costs some battery.",
            kind: Kind::Choice { options: TX_POWER.iter().map(|(o, l)| opt(o, l)).collect(), default: 7 },
            requires: None,
        },
        SettingDef {
            id: "ble_experimental_conn",
            group: Bluetooth,
            label: "Connection stability improvements",
            help: "Newer connection settings ZMK plans to make the default. Try it if the keyboard drops its connection or lags.",
            kind: toggle("CONFIG_ZMK_BLE_EXPERIMENTAL_CONN", false),
            requires: None,
        },
        SettingDef {
            id: "display",
            group: Screen,
            label: "Screen",
            help: "Turns the nice!view screens on. They use very little power, so most people leave them on.",
            kind: toggle("CONFIG_ZMK_DISPLAY", true),
            requires: None,
        },
        SettingDef {
            id: "display_blank_on_idle",
            group: Screen,
            label: "Turn the screen off when idle",
            help: "Blanks the screen while the keyboard is idle. The nice!view uses so little power that this barely helps the battery.",
            kind: toggle("CONFIG_ZMK_DISPLAY_BLANK_ON_IDLE", false),
            requires: Some("display"),
        },
        SettingDef {
            id: "nice_view_inverted",
            group: Screen,
            label: "Light text on a dark background",
            help: "Swaps the screen's colours.",
            kind: toggle("CONFIG_NICE_VIEW_WIDGET_INVERTED", false),
            requires: Some("display"),
        },
        SettingDef {
            id: "rgb",
            group: Lights,
            label: "RGB underglow",
            help: "Only useful if LEDs are fitted under your Corne. Uses a lot of battery.",
            kind: Kind::Toggle { option: "CONFIG_ZMK_RGB_UNDERGLOW", default: false, also: &["CONFIG_WS2812_STRIP"] },
            requires: None,
        },
        SettingDef {
            id: "rgb_on_start",
            group: Lights,
            label: "Lights on at start",
            help: "Whether the lights are on when the keyboard starts. You can still toggle them with a key.",
            kind: toggle("CONFIG_ZMK_RGB_UNDERGLOW_ON_START", true),
            requires: Some("rgb"),
        },
        SettingDef {
            id: "rgb_brightness",
            group: Lights,
            label: "Start brightness",
            help: "Brightness when the keyboard starts.",
            kind: Kind::Number { option: "CONFIG_ZMK_RGB_UNDERGLOW_BRT_START", default: 100, min: 0, max: 100, unit: Unit::Percent },
            requires: Some("rgb"),
        },
        SettingDef {
            id: "rgb_effect",
            group: Lights,
            label: "Start effect",
            help: "The light effect when the keyboard starts.",
            kind: Kind::Pick {
                option: "CONFIG_ZMK_RGB_UNDERGLOW_EFF_START",
                options: vec![val(0, "Solid colour"), val(1, "Breathing"), val(2, "Rainbow"), val(3, "Swirl")],
                default: 0,
            },
            requires: Some("rgb"),
        },
        SettingDef {
            id: "rgb_auto_off_idle",
            group: Lights,
            label: "Lights off when idle",
            help: "Saves battery while you're not typing.",
            kind: toggle("CONFIG_ZMK_RGB_UNDERGLOW_AUTO_OFF_IDLE", false),
            requires: Some("rgb"),
        },
        SettingDef {
            id: "rgb_auto_off_usb",
            group: Lights,
            label: "Lights off without USB power",
            help: "Lights only while a USB cable is plugged in.",
            kind: toggle("CONFIG_ZMK_RGB_UNDERGLOW_AUTO_OFF_USB", false),
            requires: Some("rgb"),
        },
        SettingDef {
            id: "hid_report",
            group: Typing,
            label: "Keys pressed at once",
            help: "“6 keys” works everywhere, including BIOS and boot menus. “Any number” (NKRO) lets you hold many keys at once, but F13–F24 then need an extra option (CONFIG_ZMK_HID_KEYBOARD_NKRO_EXTENDED_REPORT, under Advanced).",
            kind: Kind::Choice {
                options: vec![
                    opt("CONFIG_ZMK_HID_REPORT_TYPE_HKRO", "6 keys (works everywhere)"),
                    opt("CONFIG_ZMK_HID_REPORT_TYPE_NKRO", "Any number (NKRO)"),
                ],
                default: 0,
            },
            requires: None,
        },
        SettingDef {
            id: "pointing",
            group: Typing,
            label: "Mouse keys",
            help: "Lets keys move the mouse pointer, click and scroll.",
            kind: toggle("CONFIG_ZMK_POINTING", false),
            requires: None,
        },
        SettingDef {
            id: "debounce_press",
            group: Typing,
            label: "Press debounce",
            help: "How long a key must be down before it counts. Raise it if a key sometimes types twice (chatter).",
            kind: Kind::Number { option: "CONFIG_ZMK_KSCAN_DEBOUNCE_PRESS_MS", default: 5, min: 0, max: 50, unit: Unit::Milliseconds },
            requires: None,
        },
        SettingDef {
            id: "debounce_release",
            group: Typing,
            label: "Release debounce",
            help: "How long a key must be up before the release counts.",
            kind: Kind::Number { option: "CONFIG_ZMK_KSCAN_DEBOUNCE_RELEASE_MS", default: 5, min: 0, max: 50, unit: Unit::Milliseconds },
            requires: None,
        },
        SettingDef {
            id: "studio",
            group: LiveEditing,
            label: "Live editing (ZMK Studio)",
            help: "Lets apps change the keymap over USB without rebuilding. mxKeys also adds the matching build option for the left half.",
            kind: toggle("CONFIG_ZMK_STUDIO", false),
            requires: None,
        },
    ]
}

/// A setting's current value as the UI sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum Value {
    Bool(bool),
    Int(i64),
    Text(String),
    /// Index into the choice's options.
    Choice(usize),
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingView {
    pub def: SettingDef,
    pub value: Value,
    /// The file doesn't set it, so ZMK's default applies.
    pub is_default: bool,
    /// Line numbers (1-based) of the lines in effect.
    pub lines: Vec<usize>,
    /// False when the setting it depends on is off (shown greyed out).
    pub applies: bool,
    /// A value in the file the form can't show (e.g. out of range).
    pub problem: Option<String>,
}

/// What the user picked.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum NewValue {
    Bool(bool),
    Int(i64),
    Text(String),
    Choice(usize),
    /// Remove the user's value so ZMK's default applies.
    Default,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SettingsError {
    #[error("unknown setting `{0}`")]
    Unknown(String),
    #[error("that kind of value doesn't fit “{0}”")]
    WrongType(&'static str),
    #[error("“{label}” must be between {min} and {max}")]
    OutOfRange {
        label: &'static str,
        min: String,
        max: String,
    },
    #[error("“{label}” can be at most {max} bytes long (it's {len})")]
    TooLong {
        label: &'static str,
        max: usize,
        len: usize,
    },
    #[error("“{0}” can't be empty or contain line breaks or quotes")]
    BadText(&'static str),
    #[error(transparent)]
    Kconfig(#[from] crate::kconfig::KconfigError),
}

fn is_on(file: &KconfigFile, option: &str) -> Option<bool> {
    match file.get(option)? {
        KconfigValue::Bool(b) => Some(*b),
        _ => None,
    }
}

fn line_of(file: &KconfigFile, option: &str) -> Option<usize> {
    file.effective(option).map(|s| s.line)
}

pub fn show_unit(unit: Unit, stored: i64) -> String {
    match unit {
        Unit::Seconds => format!("{} s", stored as f64 / 1000.0),
        Unit::Minutes => format!("{} min", stored as f64 / 60_000.0),
        Unit::Milliseconds => format!("{stored} ms"),
        Unit::Percent => format!("{stored} %"),
    }
}

fn read(def: &SettingDef, file: &KconfigFile) -> (Value, bool, Vec<usize>, Option<String>) {
    match &def.kind {
        Kind::Toggle { option, default, .. } => match file.get(option) {
            None => (Value::Bool(*default), true, vec![], None),
            Some(KconfigValue::Bool(b)) => (
                Value::Bool(*b),
                false,
                line_of(file, option).into_iter().collect(),
                None,
            ),
            Some(other) => (
                Value::Bool(*default),
                false,
                line_of(file, option).into_iter().collect(),
                Some(format!(
                    "The file sets `{}`, which isn't on/off.",
                    other.to_conf()
                )),
            ),
        },
        Kind::Number {
            option,
            default,
            min,
            max,
            unit,
        } => match file.get(option) {
            None => (Value::Int(*default), true, vec![], None),
            Some(KconfigValue::Int(i)) => {
                let lines = line_of(file, option).into_iter().collect();
                let problem = (*i < *min || *i > *max).then(|| {
                    format!(
                        "{} is outside the usual range ({} – {}).",
                        show_unit(*unit, *i),
                        show_unit(*unit, *min),
                        show_unit(*unit, *max)
                    )
                });
                (Value::Int(*i), false, lines, problem)
            }
            Some(other) => (
                Value::Int(*default),
                false,
                line_of(file, option).into_iter().collect(),
                Some(format!(
                    "The file sets `{}`, which isn't a number.",
                    other.to_conf()
                )),
            ),
        },
        Kind::Text { option, default, .. } => match file.get(option) {
            None => (Value::Text(default.to_string()), true, vec![], None),
            Some(KconfigValue::Str(s)) => (
                Value::Text(s.clone()),
                false,
                line_of(file, option).into_iter().collect(),
                None,
            ),
            Some(other) => (
                Value::Text(default.to_string()),
                false,
                line_of(file, option).into_iter().collect(),
                Some(format!(
                    "The file sets `{}`, which isn't quoted text.",
                    other.to_conf()
                )),
            ),
        },
        Kind::Choice { options, default } => {
            // Kconfig: the last `=y` among a choice's options wins.
            let chosen = options
                .iter()
                .enumerate()
                .filter_map(|(i, o)| {
                    let s = file.effective(o.option?)?;
                    (s.state == SettingState::Active && s.value == KconfigValue::Bool(true))
                        .then_some((i, s.line))
                })
                .max_by_key(|(_, line)| *line);
            let lines: Vec<usize> = options.iter().filter_map(|o| line_of(file, o.option?)).collect();
            match chosen {
                Some((i, _)) => (Value::Choice(i), false, lines, None),
                None => (Value::Choice(*default), lines.is_empty(), lines, None),
            }
        }
        Kind::Pick {
            option,
            options,
            default,
        } => match file.get(option) {
            None => (Value::Choice(*default), true, vec![], None),
            Some(KconfigValue::Int(i)) => {
                let lines = line_of(file, option).into_iter().collect();
                match options.iter().position(|o| o.value == Some(*i)) {
                    Some(idx) => (Value::Choice(idx), false, lines, None),
                    None => (
                        Value::Choice(*default),
                        false,
                        lines,
                        Some(format!("The file sets {i}, which isn't one of the choices.")),
                    ),
                }
            }
            Some(other) => (
                Value::Choice(*default),
                false,
                line_of(file, option).into_iter().collect(),
                Some(format!(
                    "The file sets `{}`, which isn't a number.",
                    other.to_conf()
                )),
            ),
        },
    }
}

/// All catalog settings with their current values in `file`.
pub fn view(file: &KconfigFile) -> Vec<SettingView> {
    let defs = catalog();
    let values: Vec<_> = defs.iter().map(|d| read(d, file)).collect();
    defs.iter()
        .zip(&values)
        .map(|(def, (value, is_default, lines, problem))| {
            let applies = def.requires.is_none_or(|req| {
                defs.iter()
                    .position(|d| d.id == req)
                    .is_none_or(|i| values[i].0 == Value::Bool(true))
            });
            SettingView {
                def: def.clone(),
                value: value.clone(),
                is_default: *is_default,
                lines: lines.clone(),
                applies,
                problem: problem.clone(),
            }
        })
        .collect()
}

/// Kconfig options the catalog covers (the Advanced view lists the rest as "other").
pub fn known_options() -> Vec<&'static str> {
    catalog().iter().flat_map(|d| d.options()).collect()
}

/// Comment out every line that sets `option` (so its default applies).
fn comment_out_all(file: &KconfigFile, option: &str) -> Vec<Patch> {
    file.settings
        .iter()
        .filter(|s| s.name == option && s.state != SettingState::CommentedOut)
        .map(|s| {
            let line = &file.source()[s.line_span.range()];
            let indent = line.len() - line.trim_start().len();
            match s.state {
                SettingState::NotSet => {
                    Patch::replace(s.line_span, format!("{}# {option}=n", &line[..indent]))
                }
                _ => Patch::insert(s.line_span.start + indent, "# "),
            }
        })
        .collect()
}

/// The edits that set the setting `id` to `new`.
pub fn patches(file: &KconfigFile, id: &str, new: &NewValue) -> Result<Vec<Patch>, SettingsError> {
    let defs = catalog();
    let def = defs
        .iter()
        .find(|d| d.id == id)
        .ok_or_else(|| SettingsError::Unknown(id.into()))?;
    if *new == NewValue::Default {
        return Ok(def
            .options()
            .iter()
            .flat_map(|o| comment_out_all(file, o))
            .collect());
    }
    let wrong = || SettingsError::WrongType(def.label);
    match (&def.kind, new) {
        (Kind::Toggle { option, also, .. }, NewValue::Bool(on)) => {
            let mut out = vec![file.set(option, &KconfigValue::Bool(*on))?];
            if *on {
                for o in *also {
                    if is_on(file, o) != Some(true) {
                        out.push(file.set(o, &KconfigValue::Bool(true))?);
                    }
                }
            }
            Ok(out)
        }
        (
            Kind::Number {
                option,
                min,
                max,
                unit,
                ..
            },
            NewValue::Int(i),
        ) => {
            if i < min || i > max {
                return Err(SettingsError::OutOfRange {
                    label: def.label,
                    min: show_unit(*unit, *min),
                    max: show_unit(*unit, *max),
                });
            }
            Ok(vec![file.set(option, &KconfigValue::Int(*i))?])
        }
        (
            Kind::Text {
                option, max_bytes, ..
            },
            NewValue::Text(t),
        ) => {
            if t.trim().is_empty() || t.chars().any(|c| c.is_control() || c == '"' || c == '\\') {
                return Err(SettingsError::BadText(def.label));
            }
            if t.len() > *max_bytes {
                return Err(SettingsError::TooLong {
                    label: def.label,
                    max: *max_bytes,
                    len: t.len(),
                });
            }
            Ok(vec![file.set(option, &KconfigValue::Str(t.clone()))?])
        }
        (Kind::Choice { options, .. }, NewValue::Choice(idx)) => {
            let chosen = options.get(*idx).and_then(|o| o.option).ok_or_else(wrong)?;
            let mut out = vec![file.set(chosen, &KconfigValue::Bool(true))?];
            for o in options.iter().filter_map(|o| o.option).filter(|o| *o != chosen) {
                if is_on(file, o) == Some(true) {
                    out.extend(comment_out_all(file, o));
                }
            }
            Ok(out)
        }
        (Kind::Pick { option, options, .. }, NewValue::Choice(idx)) => {
            let v = options.get(*idx).and_then(|o| o.value).ok_or_else(wrong)?;
            Ok(vec![file.set(option, &KconfigValue::Int(v))?])
        }
        _ => Err(wrong()),
    }
}

/// Apply `new` to setting `id` and return the new file text.
pub fn set(file: &KconfigFile, id: &str, new: &NewValue) -> Result<KconfigFile, SettingsError> {
    let p = patches(file, id, new)?;
    Ok(file.apply(&p)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    const OWNER: &str = include_str!("../tests/fixtures/xnmrph_corne.conf");

    fn get<'a>(v: &'a [SettingView], id: &str) -> &'a SettingView {
        v.iter().find(|s| s.def.id == id).unwrap()
    }

    #[test]
    fn catalog_is_consistent() {
        let defs = catalog();
        for d in &defs {
            assert_eq!(defs.iter().filter(|e| e.id == d.id).count(), 1, "{}", d.id);
            for o in d.options() {
                assert!(o.starts_with("CONFIG_"), "{o}");
            }
            if let Some(r) = d.requires {
                assert!(matches!(
                    defs.iter().find(|e| e.id == r).unwrap().kind,
                    Kind::Toggle { .. }
                ));
            }
        }
        for g in Group::ALL {
            assert!(defs.iter().any(|d| d.group == g));
        }
    }

    #[test]
    fn reads_the_owners_conf() {
        let v = view(&KconfigFile::parse(OWNER));
        assert_eq!(get(&v, "display").value, Value::Bool(false));
        assert!(!get(&v, "display").is_default);
        assert!(!get(&v, "nice_view_inverted").applies);
        assert_eq!(get(&v, "sleep").value, Value::Bool(true));
        assert_eq!(get(&v, "sleep_timeout").value, Value::Int(900_000));
        assert!(get(&v, "sleep_timeout").is_default && get(&v, "sleep_timeout").applies);
        assert_eq!(get(&v, "tx_power").value, Value::Choice(0));
        assert_eq!(get(&v, "keyboard_name").value, Value::Text("Corne xnmrph".into()));
        assert_eq!(get(&v, "hid_report").value, Value::Choice(0));
        assert!(get(&v, "hid_report").is_default);
        // `#CONFIG_ZMK_RGB_UNDERGLOW=y` is commented out → default (off).
        assert_eq!(get(&v, "rgb").value, Value::Bool(false));
        assert!(get(&v, "rgb").is_default && !get(&v, "rgb_effect").applies);
    }

    #[test]
    fn turning_the_screen_on_changes_one_line() {
        let f = KconfigFile::parse(OWNER);
        let g = set(&f, "display", &NewValue::Bool(true)).unwrap();
        assert_eq!(
            g.source(),
            OWNER.replace("CONFIG_ZMK_DISPLAY=n", "CONFIG_ZMK_DISPLAY=y")
        );
        let h = set(&f, "display", &NewValue::Default).unwrap();
        assert_eq!(
            h.source(),
            OWNER.replace("CONFIG_ZMK_DISPLAY=n", "# CONFIG_ZMK_DISPLAY=n")
        );
        assert_eq!(get(&view(&h), "display").value, Value::Bool(true));
    }

    #[test]
    fn rgb_uncomments_its_companions() {
        let f = KconfigFile::parse(OWNER);
        let g = set(&f, "rgb", &NewValue::Bool(true)).unwrap();
        assert_eq!(
            g.source(),
            OWNER
                .replace("#CONFIG_ZMK_RGB_UNDERGLOW=y", "CONFIG_ZMK_RGB_UNDERGLOW=y")
                .replace("#CONFIG_WS2812_STRIP=y", "CONFIG_WS2812_STRIP=y")
        );
        let g = set(&g, "rgb_effect", &NewValue::Choice(2)).unwrap();
        assert!(g.source().contains("\nCONFIG_ZMK_RGB_UNDERGLOW_EFF_START=2\n"));
        assert_eq!(get(&view(&g), "rgb_effect").value, Value::Choice(2));
        // Turning it off leaves the LED driver line alone.
        let h = set(&g, "rgb", &NewValue::Bool(false)).unwrap();
        assert!(
            h.source().contains("CONFIG_ZMK_RGB_UNDERGLOW=n")
                && h.source().contains("\nCONFIG_WS2812_STRIP=y")
        );
    }

    #[test]
    fn choices_replace_the_other_option() {
        let f = KconfigFile::parse(OWNER);
        let g = set(&f, "tx_power", &NewValue::Choice(7)).unwrap();
        assert!(g.source().contains("# CONFIG_BT_CTLR_TX_PWR_PLUS_8=y\n"));
        assert!(g
            .source()
            .ends_with("CONFIG_ZMK_POINTING=y\n\nCONFIG_BT_CTLR_TX_PWR_0=y\n"));
        assert_eq!(get(&view(&g), "tx_power").value, Value::Choice(7));
        let h = set(&g, "tx_power", &NewValue::Default).unwrap();
        assert!(get(&view(&h), "tx_power").is_default);
        // Picking +8 again uncomments the user's own line instead of appending.
        let i = set(&h, "tx_power", &NewValue::Choice(0)).unwrap();
        assert!(i.source().contains("\nCONFIG_BT_CTLR_TX_PWR_PLUS_8=y\n"));
        assert_eq!(get(&view(&i), "tx_power").value, Value::Choice(0));
    }

    #[test]
    fn numbers_and_text_are_validated() {
        let f = KconfigFile::parse(OWNER);
        let g = set(&f, "sleep_timeout", &NewValue::Int(1_800_000)).unwrap();
        assert!(g.source().ends_with("CONFIG_ZMK_IDLE_SLEEP_TIMEOUT=1800000\n"));
        assert!(matches!(
            set(&f, "sleep_timeout", &NewValue::Int(10)),
            Err(SettingsError::OutOfRange { .. })
        ));
        assert!(matches!(
            set(
                &f,
                "keyboard_name",
                &NewValue::Text("A very long keyboard name".into())
            ),
            Err(SettingsError::TooLong { max: 16, .. })
        ));
        assert!(matches!(
            set(&f, "keyboard_name", &NewValue::Text("a\"b".into())),
            Err(SettingsError::BadText(_))
        ));
        let g = set(&f, "keyboard_name", &NewValue::Text("Corne ✓".into())).unwrap();
        assert!(g.source().contains("CONFIG_ZMK_KEYBOARD_NAME=\"Corne ✓\"\n"));
        assert_eq!(
            set(&f, "sleep", &NewValue::Int(1)).unwrap_err(),
            SettingsError::WrongType("Deep sleep when unused")
        );
        assert!(matches!(
            set(&f, "nope", &NewValue::Default),
            Err(SettingsError::Unknown(_))
        ));
    }

    #[test]
    fn default_comments_out_duplicates_and_problems_are_reported() {
        let f = KconfigFile::parse("CONFIG_ZMK_SLEEP=y\nCONFIG_ZMK_SLEEP=y\nCONFIG_ZMK_IDLE_TIMEOUT=abc\n");
        let v = view(&f);
        assert!(get(&v, "idle_timeout").problem.is_some());
        let g = set(&f, "sleep", &NewValue::Default).unwrap();
        assert_eq!(
            g.source(),
            "# CONFIG_ZMK_SLEEP=y\n# CONFIG_ZMK_SLEEP=y\nCONFIG_ZMK_IDLE_TIMEOUT=abc\n"
        );
    }
}
