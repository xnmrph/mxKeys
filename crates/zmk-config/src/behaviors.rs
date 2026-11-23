//! Typed view and editing of the keymap's own definitions: hold-taps, mod-morphs,
//! tap-dances, sticky keys, caps word, macros, combos, conditional layers, tuned
//! built-ins (`&mt { … };`) and — read-only — zmk-helpers macros (`ZMK_HOLD_TAP(…)`).
//!
//! The model is schema-driven: each definition type lists its known properties with a
//! type (time, flag, choice, bindings, key positions, layers, modifiers, …). The UI
//! renders forms from the schema; edits are property-level byte patches (set / insert /
//! remove a property) plus adding or removing whole definition nodes, so everything the
//! user didn't touch keeps its formatting and comments.
//!
//! Property names, types and defaults follow ZMK's devicetree bindings
//! (`app/dts/bindings/**/*.yaml` at 4de3133) and Kconfig defaults.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::catalog;
use crate::dts::{CellKind, Document, Item, Node, Patch, Property, RefNode, Span};
use crate::keymap::Keymap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DefType {
    HoldTap,
    ModMorph,
    TapDance,
    StickyKey,
    CapsWord,
    Macro,
    Combo,
    ConditionalLayer,
    /// Any other behavior (key repeat, custom modules …): properties shown raw.
    Other,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PropType {
    /// `<200>` milliseconds.
    Ms,
    /// Boolean property (`retro-tap;`).
    Flag,
    /// `"string"` from a fixed list: (value, plain label).
    Choice {
        options: Vec<(&'static str, &'static str)>,
    },
    /// Behavior bindings. `bare`: behaviors without parameters (`<&kp>, <&kp>`);
    /// `count`: fixed number of entries.
    Bindings { count: Option<usize>, bare: bool },
    /// `<(MOD_LSFT|MOD_RSFT)>`.
    Mods,
    /// Key positions (`<13 29>`).
    Positions,
    /// Layer list (`<1 2>` or `#define` names).
    Layers,
    /// One layer.
    Layer,
    /// Keycode list (`<MINUS BSPC>`).
    Keys,
}

#[derive(Debug, Clone, Serialize)]
pub struct PropSchema {
    pub name: &'static str,
    pub label: &'static str,
    pub help: &'static str,
    #[serde(flatten)]
    pub ty: PropType,
    pub required: bool,
    /// What applies when the property is absent (plain text, for the UI).
    pub default: Option<&'static str>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Schema {
    pub def_type: DefType,
    pub compatible: Option<&'static str>,
    pub title: &'static str,
    pub help: &'static str,
    pub binding_cells: Option<u8>,
    pub props: Vec<PropSchema>,
}

fn prop(
    name: &'static str,
    label: &'static str,
    help: &'static str,
    ty: PropType,
    default: Option<&'static str>,
) -> PropSchema {
    PropSchema {
        name,
        label,
        help,
        ty,
        required: false,
        default,
    }
}

fn required(mut p: PropSchema) -> PropSchema {
    p.required = true;
    p
}

const FLAVORS: [(&str, &str); 4] = [
    (
        "hold-preferred",
        "Hold preferred — hold as soon as the time is up or another key is pressed",
    ),
    (
        "balanced",
        "Balanced — hold if another key is pressed and released while held",
    ),
    (
        "tap-preferred",
        "Tap preferred — hold only when held past the time",
    ),
    (
        "tap-unless-interrupted",
        "Tap unless interrupted — hold only if another key is pressed first",
    ),
];

pub fn schema(t: DefType) -> Schema {
    use PropType::*;
    let term = |d: &'static str| {
        prop(
            "tapping-term-ms",
            "Decision time",
            "How long a key must be held to count as a hold.",
            Ms,
            Some(d),
        )
    };
    match t {
        DefType::HoldTap => Schema {
            def_type: t,
            compatible: Some("zmk,behavior-hold-tap"),
            title: "Hold-tap",
            help: "One key, two jobs: something when tapped, something else when held (home-row mods, layer-tap).",
            binding_cells: Some(2),
            props: vec![
                required(prop("bindings", "Hold / tap", "What holding and tapping do (behaviors without parameters; the key sets them).", Bindings { count: Some(2), bare: true }, None)),
                term("none — must be set"),
                prop("flavor", "When does it count as a hold?", "How a press of another key while this one is down is decided.", Choice { options: FLAVORS.to_vec() }, Some("hold-preferred")),
                prop("quick-tap-ms", "Quick tap", "Tapping again within this time and holding repeats the tap instead of holding (e.g. hold-to-repeat a letter).", Ms, Some("off")),
                prop("require-prior-idle-ms", "Typing streak guard", "If another key was pressed within this time, it's always a tap — prevents accidental mods while typing fast.", Ms, Some("off")),
                prop("hold-trigger-key-positions", "Hold only with these keys", "If set, a hold is only chosen when the other key pressed is one of these positions (e.g. the opposite hand).", Positions, Some("any key")),
                prop("hold-trigger-on-release", "Decide on release", "Wait for the other key's release before applying the position rule above (allows combining mods on the same hand).", Flag, Some("off")),
                prop("retro-tap", "Tap if held alone", "Holding past the time and releasing without pressing another key still sends the tap.", Flag, Some("off")),
                prop("hold-while-undecided", "Hold while undecided", "Press the hold behavior immediately and release it if it turns out to be a tap.", Flag, Some("off")),
                prop("hold-while-undecided-linger", "Keep hold until tap is done", "With \"hold while undecided\", keep it pressed until the tap has been sent.", Flag, Some("off")),
            ],
        },
        DefType::ModMorph => Schema {
            def_type: t,
            compatible: Some("zmk,behavior-mod-morph"),
            title: "Shift-dependent key",
            help: "Does one thing normally and another while certain modifiers are held (e.g. a different symbol with Shift).",
            binding_cells: Some(0),
            props: vec![
                required(prop("bindings", "Normal / with modifier", "What the key does without and with the modifiers.", Bindings { count: Some(2), bare: false }, None)),
                required(prop("mods", "Modifiers", "Which modifiers switch to the second binding.", Mods, None)),
                prop("keep-mods", "Keep modifiers", "Modifiers that stay pressed for the second binding (by default they are released).", Mods, Some("none")),
            ],
        },
        DefType::TapDance => Schema {
            def_type: t,
            compatible: Some("zmk,behavior-tap-dance"),
            title: "Tap dance",
            help: "Different results for one, two, three … taps in a row.",
            binding_cells: Some(0),
            props: vec![
                required(prop("bindings", "Taps", "First entry for one tap, second for two taps, and so on.", Bindings { count: None, bare: false }, None)),
                term("200 ms"),
            ],
        },
        DefType::StickyKey => Schema {
            def_type: t,
            compatible: Some("zmk,behavior-sticky-key"),
            title: "Sticky key",
            help: "Stays active until the next key press (one-shot modifier or layer).",
            binding_cells: Some(1),
            props: vec![
                required(prop("bindings", "Behavior", "What is kept active, e.g. &kp for a modifier or &mo for a layer.", Bindings { count: Some(1), bare: true }, None)),
                required(prop("release-after-ms", "Give up after", "Deactivates if no other key is pressed within this time.", Ms, None)),
                prop("quick-release", "Release on next key press", "Release as soon as the next key goes down instead of when it goes up.", Flag, Some("off")),
                prop("lazy", "Lazy", "Only activate right before the next key press.", Flag, Some("off")),
                prop("ignore-modifiers", "Ignore modifiers", "Pressing a modifier doesn't use up the sticky key.", Flag, Some("off")),
            ],
        },
        DefType::CapsWord => Schema {
            def_type: t,
            compatible: Some("zmk,behavior-caps-word"),
            title: "Caps word",
            help: "Types in capitals until a key that isn't a letter (or in the list) is pressed.",
            binding_cells: Some(0),
            props: vec![
                prop("continue-list", "Keep going with", "Keys that don't end caps word.", Keys, Some("UNDERSCORE BACKSPACE DELETE")),
                prop("mods", "Modifiers applied", "Modifiers added to letters.", Mods, Some("Left Shift")),
            ],
        },
        DefType::Macro => Schema {
            def_type: t,
            compatible: Some("zmk,behavior-macro"),
            title: "Macro",
            help: "Plays a sequence of key presses.",
            binding_cells: Some(0),
            props: vec![
                required(prop("bindings", "Steps", "Played in order. Tap/Press/Release switch how the following keys are sent.", Bindings { count: None, bare: false }, None)),
                prop("wait-ms", "Pause between steps", "Time between steps.", Ms, Some("15 ms")),
                prop("tap-ms", "Tap length", "How long each tapped key is held down.", Ms, Some("30 ms")),
            ],
        },
        DefType::Combo => Schema {
            def_type: t,
            compatible: None,
            title: "Combo",
            help: "Press several keys together to get something else.",
            binding_cells: None,
            props: vec![
                required(prop("key-positions", "Keys", "The keys to press together.", Positions, None)),
                required(prop("bindings", "Result", "What the combo does.", Bindings { count: Some(1), bare: false }, None)),
                prop("timeout-ms", "Press within", "All keys must go down within this time.", Ms, Some("50 ms")),
                prop("layers", "Only on layers", "Leave empty for all layers.", Layers, Some("all layers")),
                prop("require-prior-idle-ms", "Typing streak guard", "Ignore the combo if another key was pressed within this time.", Ms, Some("off")),
                prop("slow-release", "Release with the last key", "Keep the result active until all combo keys are released.", Flag, Some("off")),
            ],
        },
        DefType::ConditionalLayer => Schema {
            def_type: t,
            compatible: None,
            title: "Conditional layer",
            help: "Turns a layer on automatically while all of some other layers are on (e.g. Nav + Sym → Adjust).",
            binding_cells: None,
            props: vec![
                required(prop("if-layers", "When all of these are on", "", Layers, None)),
                required(prop("then-layer", "Turn on", "", Layer, None)),
            ],
        },
        DefType::Other => Schema {
            def_type: t,
            compatible: None,
            title: "Other behavior",
            help: "A behavior type mxKeys doesn't have a form for yet; its settings are shown as written.",
            binding_cells: None,
            props: vec![],
        },
    }
}

fn type_of_compatible(c: &str) -> Option<DefType> {
    Some(match c {
        "zmk,behavior-hold-tap" => DefType::HoldTap,
        "zmk,behavior-mod-morph" => DefType::ModMorph,
        "zmk,behavior-tap-dance" => DefType::TapDance,
        "zmk,behavior-sticky-key" => DefType::StickyKey,
        "zmk,behavior-caps-word" => DefType::CapsWord,
        c if c.starts_with("zmk,behavior-macro") => DefType::Macro,
        c if c.starts_with("zmk,behavior-") => DefType::Other,
        _ => return None,
    })
}

/// Schema type of a built-in behavior that can be tuned with `&label { … };`.
pub fn builtin_type(label: &str) -> Option<DefType> {
    Some(match label {
        "mt" | "lt" => DefType::HoldTap,
        "sk" | "sl" => DefType::StickyKey,
        "caps_word" => DefType::CapsWord,
        _ => return None,
    })
}

/// A property's value as the UI sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum PropValue {
    Int(i64),
    Flag(bool),
    Text(String),
    Bindings(Vec<String>),
    /// Key positions.
    Numbers(Vec<i64>),
    /// Layer indices.
    Layers(Vec<usize>),
    /// Modifier or keycode names.
    Names(Vec<String>),
    /// Written in a way the form can't show (e.g. a `#define`); kept as is.
    Raw(String),
}

#[derive(Debug, Clone, Serialize)]
pub struct PropView {
    pub name: String,
    /// `None` when the property isn't in the file (its default applies).
    pub value: Option<PropValue>,
    /// The value text as written.
    pub raw: Option<String>,
    pub line: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// A node in a container (`behaviors`, `macros`, `combos`, `conditional_layers`).
    Node,
    /// `&mt { … };` tuning a built-in.
    Override,
    /// A zmk-helpers macro call (read-only for now).
    Helper,
}

#[derive(Debug, Clone, Serialize)]
pub struct Definition {
    /// Stable id for edits: `container/node-name`, `&label` for overrides, or
    /// `helper:<index>`.
    pub id: String,
    pub def_type: DefType,
    pub source: Source,
    /// The `&label` keys use, if any.
    pub label: Option<String>,
    pub node_name: String,
    pub title: String,
    pub compatible: Option<String>,
    /// Typed properties in schema order (absent ones included), then the rest.
    pub props: Vec<PropView>,
    /// Properties not in the schema (`(name, raw value)`), shown under Advanced.
    pub other_props: Vec<(String, String)>,
    /// How many times `&label` is used elsewhere in the keymap file.
    pub uses: usize,
    /// Why the app can't edit it (helper macros, macro-made values …).
    pub read_only: Option<String>,
    pub line: usize,
}

/// What the user picked for a property.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub enum NewProp {
    Int(i64),
    Flag(bool),
    Text(String),
    Bindings(Vec<String>),
    Numbers(Vec<i64>),
    Layers(Vec<usize>),
    Names(Vec<String>),
    /// Remove the property (its default applies).
    Remove,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DefError {
    #[error("couldn't find “{0}” in the keymap (was it changed outside mxKeys?)")]
    NotFound(String),
    #[error("{0} was written with a helper macro; edit it in a text editor for now")]
    ReadOnly(String),
    #[error("“{0}” is used {1} time(s) in your keymap — remove those keys first")]
    InUse(String, usize),
    #[error("“{0}” isn't a valid name: use lowercase letters, digits and _ (starting with a letter)")]
    BadName(String),
    #[error("the name “{0}” is already taken")]
    NameTaken(String),
    #[error("“{label}” {problem}")]
    BadValue { label: String, problem: String },
    #[error("there is no layer {0}")]
    NoSuchLayer(usize),
    #[error("{0} can't be tuned here")]
    NotTunable(String),
}

// ---------------------------------------------------------------- reading

/// A node-like body we can edit (a `Node` or a `RefNode`).
#[derive(Clone, Copy)]
struct Body<'a> {
    span: Span,
    close_brace: Span,
    children: &'a [Item],
}

impl<'a> Body<'a> {
    fn of_node(n: &'a Node) -> Self {
        Body {
            span: n.span,
            close_brace: n.close_brace,
            children: &n.children,
        }
    }
    fn of_ref(n: &'a RefNode) -> Self {
        Body {
            span: n.span,
            close_brace: n.close_brace,
            children: &n.children,
        }
    }
    fn property(&self, name: &str) -> Option<&'a Property> {
        self.children.iter().find_map(|c| match c {
            Item::Property(p) if p.name == name => Some(p),
            _ => None,
        })
    }
    fn properties(&self) -> impl Iterator<Item = &'a Property> {
        self.children.iter().filter_map(|c| match c {
            Item::Property(p) => Some(p),
            _ => None,
        })
    }
}

/// Every editable/readable definition with its container name.
enum Found<'a> {
    Node {
        parent: &'a str,
        node: &'a Node,
        ty: DefType,
    },
    Override {
        node: &'a RefNode,
        ty: DefType,
    },
    Helper {
        index: usize,
        call: &'a crate::dts::MacroCall,
        ty: DefType,
    },
}

fn collect<'a>(doc: &'a Document) -> Vec<Found<'a>> {
    fn visit<'a>(items: &'a [Item], parent: Option<&'a Node>, out: &mut Vec<Found<'a>>) {
        for item in items {
            match item {
                Item::Node(n) => {
                    let parent_compat = parent.and_then(|p| p.compatible());
                    let ty = match (n.compatible(), parent_compat) {
                        (_, Some("zmk,combos")) => Some(DefType::Combo),
                        (_, Some("zmk,conditional-layers")) => Some(DefType::ConditionalLayer),
                        (Some(c), _) => type_of_compatible(c),
                        _ => None,
                    };
                    match ty {
                        Some(ty) => out.push(Found::Node {
                            parent: parent.map_or("/", |p| p.name.as_str()),
                            node: n,
                            ty,
                        }),
                        None => visit(&n.children, Some(n), out),
                    }
                }
                Item::RefNode(r) => {
                    if let Some(ty) = builtin_type(&r.target) {
                        out.push(Found::Override { node: r, ty });
                    }
                }
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    visit(&doc.items, None, &mut out);
    let mut index = 0;
    for item in &doc.items {
        if let Item::MacroCall(call) = item {
            if let Some(ty) = helper_type(&call.name) {
                out.push(Found::Helper { index, call, ty });
            }
            index += 1;
        }
    }
    out
}

fn helper_type(name: &str) -> Option<DefType> {
    Some(match name {
        "ZMK_HOLD_TAP" => DefType::HoldTap,
        "ZMK_MOD_MORPH" => DefType::ModMorph,
        "ZMK_TAP_DANCE" => DefType::TapDance,
        "ZMK_STICKY_KEY" => DefType::StickyKey,
        "ZMK_CAPS_WORD" => DefType::CapsWord,
        "ZMK_MACRO" | "ZMK_MACRO_ONE_PARAM" | "ZMK_MACRO_TWO_PARAM" => DefType::Macro,
        "ZMK_COMBO" => DefType::Combo,
        "ZMK_CONDITIONAL_LAYER" => DefType::ConditionalLayer,
        "ZMK_BEHAVIOR" => DefType::Other,
        _ => return None,
    })
}

struct Ctx {
    /// `#define NAME <number>`.
    numbers: HashMap<String, i64>,
    /// layer index → name to write (`#define` if there is one).
    layer_names: Vec<(usize, Option<String>)>,
}

impl Ctx {
    fn new(doc: &Document, km: Option<&Keymap>) -> Ctx {
        let numbers = doc
            .defines()
            .filter(|(_, params, _)| params.is_none())
            .filter_map(|(n, _, body)| Some((n.to_string(), body.trim().parse::<i64>().ok()?)))
            .collect();
        let layer_names = km
            .map(|km| {
                catalog::layer_refs(doc, km)
                    .into_iter()
                    .map(|r| (r.index, r.define))
                    .collect()
            })
            .unwrap_or_default();
        Ctx { numbers, layer_names }
    }

    fn number(&self, kind: &CellKind) -> Option<i64> {
        match kind {
            CellKind::Number(n) => parse_int(n),
            CellKind::Ident(name) => self.numbers.get(name).copied(),
            _ => None,
        }
    }
}

fn parse_int(s: &str) -> Option<i64> {
    match s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        Some(h) => i64::from_str_radix(h, 16).ok(),
        None => s.parse().ok(),
    }
}

/// Spans of each binding (`&x params…`) in a property, in order.
fn binding_spans(p: &Property) -> Option<Vec<Span>> {
    let mut out: Vec<Span> = Vec::new();
    for c in p.cells() {
        match &c.kind {
            CellKind::Ref(_) => out.push(c.span),
            _ => {
                let last = out.last_mut()?;
                *last = last.to(c.span);
            }
        }
    }
    Some(out)
}

fn read_value(doc: &Document, ctx: &Ctx, p: &Property, ty: &PropType) -> PropValue {
    let raw = || PropValue::Raw(p.values_span.map(|s| doc.text(s).to_string()).unwrap_or_default());
    let cells: Vec<_> = p.cells().collect();
    let all_cells = p
        .values
        .iter()
        .all(|v| matches!(v, crate::dts::Value::Cells { .. }));
    match ty {
        PropType::Flag => PropValue::Flag(true),
        PropType::Choice { .. } => p
            .first_string()
            .map(|s| PropValue::Text(s.into()))
            .unwrap_or_else(raw),
        _ if !all_cells => raw(),
        PropType::Ms => match cells.as_slice() {
            [c] => ctx.number(&c.kind).map(PropValue::Int).unwrap_or_else(raw),
            _ => raw(),
        },
        PropType::Bindings { .. } => match binding_spans(p) {
            Some(spans) if !spans.is_empty() => {
                PropValue::Bindings(spans.iter().map(|s| normalize_ws(doc.text(*s))).collect())
            }
            _ => raw(),
        },
        PropType::Positions => cells
            .iter()
            .map(|c| ctx.number(&c.kind))
            .collect::<Option<Vec<_>>>()
            .map(PropValue::Numbers)
            .unwrap_or_else(raw),
        PropType::Layers | PropType::Layer => cells
            .iter()
            .map(|c| ctx.number(&c.kind).and_then(|n| usize::try_from(n).ok()))
            .collect::<Option<Vec<_>>>()
            .map(PropValue::Layers)
            .unwrap_or_else(raw),
        PropType::Keys => cells
            .iter()
            .map(|c| match c.kind {
                CellKind::Ident(_) | CellKind::Call { .. } => Some(doc.text(c.span).to_string()),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()
            .map(PropValue::Names)
            .unwrap_or_else(raw),
        PropType::Mods => {
            let text = p.values_span.map(|s| doc.text(s)).unwrap_or_default();
            let inner = text.trim().trim_start_matches('<').trim_end_matches('>').trim();
            let inner = inner.trim_start_matches('(').trim_end_matches(')');
            let names: Vec<String> = inner.split('|').map(|s| s.trim().to_string()).collect();
            if names
                .iter()
                .all(|n| n.starts_with("MOD_") && n[4..].bytes().all(|b| b.is_ascii_uppercase()))
            {
                PropValue::Names(names)
            } else {
                raw()
            }
        }
    }
}

fn normalize_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn title_of(body: Body, fallback: &str) -> String {
    body.property("display-name")
        .or_else(|| body.property("label"))
        .and_then(|p| p.first_string())
        .map(str::to_string)
        .unwrap_or_else(|| catalog::prettify(fallback))
}

fn line_of(doc: &Document, pos: usize) -> usize {
    doc.source()[..pos].matches('\n').count() + 1
}

/// How often `&label` appears outside `exclude` (in cells and `#define` bodies).
fn count_uses(doc: &Document, label: &str, exclude: Span) -> usize {
    let mut n = 0;
    for item in doc.walk() {
        match item {
            Item::Property(p) if !exclude.contains(p.span) => {
                n += p
                    .cells()
                    .filter(|c| matches!(&c.kind, CellKind::Ref(l) if l == label))
                    .count();
            }
            Item::Directive(crate::dts::Directive {
                kind: crate::dts::DirectiveKind::Define { body, .. },
                ..
            }) => {
                n += body
                    .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '&'))
                    .filter(|w| w.strip_prefix('&') == Some(label))
                    .count();
            }
            _ => {}
        }
    }
    n
}

fn view_of_body(
    doc: &Document,
    ctx: &Ctx,
    body: Body,
    ty: DefType,
) -> (Vec<PropView>, Vec<(String, String)>) {
    let schema = schema(ty);
    let mut props = Vec::new();
    for s in &schema.props {
        let p = body.property(s.name);
        props.push(PropView {
            name: s.name.to_string(),
            value: p.map(|p| read_value(doc, ctx, p, &s.ty)),
            raw: p.and_then(|p| p.values_span.map(|v| doc.text(v).to_string())),
            line: p.map(|p| line_of(doc, p.span.start)),
        });
    }
    let hidden = ["compatible", "#binding-cells", "label", "display-name"];
    let other = body
        .properties()
        .filter(|p| !schema.props.iter().any(|s| s.name == p.name) && !hidden.contains(&p.name.as_str()))
        .map(|p| {
            (
                p.name.clone(),
                p.values_span.map(|v| doc.text(v).to_string()).unwrap_or_default(),
            )
        })
        .collect();
    (props, other)
}

/// Parse a helper macro call into a temporary document with one node.
fn helper_doc(call: &crate::dts::MacroCall, ty: DefType) -> Option<(String, Document)> {
    let name = call.args.first()?.trim().to_string();
    let rest = &call.args[1..];
    let body = match ty {
        DefType::Combo => {
            let mut s = format!(
                "bindings = <{}>; key-positions = <{}>;",
                rest.first()?,
                rest.get(1)?
            );
            if let Some(l) = rest.get(2).filter(|l| !l.trim().is_empty()) {
                s += &format!(" layers = <{l}>;");
            }
            if let Some(t) = rest.get(3) {
                s += &format!(" timeout-ms = <{t}>;");
            }
            if let Some(t) = rest.get(4) {
                s += &format!(" require-prior-idle-ms = <{t}>;");
            }
            s
        }
        DefType::ConditionalLayer => format!(
            "if-layers = <{}>; then-layer = <{}>;",
            rest.first()?,
            rest.get(1)?
        ),
        DefType::Other => rest.get(1..).map(|r| r.join(",")).unwrap_or_default(),
        _ => rest.join(","),
    };
    Some((name, Document::parse(format!("/ {{ helper {{ {body} }}; }};"))))
}

/// All definitions in the keymap document.
pub fn list(doc: &Document, km: Option<&Keymap>) -> Vec<Definition> {
    let ctx = Ctx::new(doc, km);
    let mut out = Vec::new();
    for f in collect(doc) {
        match f {
            Found::Node { parent, node, ty } => {
                let body = Body::of_node(node);
                let (props, other_props) = view_of_body(doc, &ctx, body, ty);
                let label = node.labels.first().map(|l| l.name.clone());
                out.push(Definition {
                    id: format!("{parent}/{}", node.name),
                    def_type: ty,
                    source: Source::Node,
                    uses: label.as_deref().map_or(0, |l| count_uses(doc, l, node.span)),
                    title: title_of(body, label.as_deref().unwrap_or(&node.name)),
                    label,
                    node_name: node.name.clone(),
                    compatible: node.compatible().map(str::to_string),
                    props,
                    other_props,
                    read_only: None,
                    line: line_of(doc, node.span.start),
                });
            }
            Found::Override { node, ty } => {
                let body = Body::of_ref(node);
                let (props, other_props) = view_of_body(doc, &ctx, body, ty);
                out.push(Definition {
                    id: format!("&{}", node.target),
                    def_type: ty,
                    source: Source::Override,
                    label: Some(node.target.clone()),
                    node_name: node.target.clone(),
                    title: format!("Built-in &{}", node.target),
                    compatible: None,
                    props,
                    other_props,
                    uses: count_uses(doc, &node.target, node.span),
                    read_only: None,
                    line: line_of(doc, node.span.start),
                });
            }
            Found::Helper { index, call, ty } => {
                let Some((name, hdoc)) = helper_doc(call, ty) else {
                    continue;
                };
                let hctx = Ctx {
                    numbers: ctx.numbers.clone(),
                    layer_names: ctx.layer_names.clone(),
                };
                let Some(node) = hdoc.walk().find_map(|i| match i {
                    Item::Node(n) if n.name == "helper" => Some(n),
                    _ => None,
                }) else {
                    continue;
                };
                let (props, other_props) = view_of_body(&hdoc, &hctx, Body::of_node(node), ty);
                let label = matches!(ty, DefType::Combo | DefType::ConditionalLayer)
                    .then_some(None)
                    .unwrap_or(Some(name.clone()));
                out.push(Definition {
                    id: format!("helper:{index}"),
                    def_type: ty,
                    source: Source::Helper,
                    uses: label.as_deref().map_or(0, |l| count_uses(doc, l, call.span)),
                    label,
                    title: catalog::prettify(&name),
                    node_name: name,
                    compatible: None,
                    props: props.into_iter().map(|p| PropView { line: None, ..p }).collect(),
                    other_props,
                    read_only: Some(format!("written with the {} helper macro", call.name)),
                    line: line_of(doc, call.span.start),
                });
            }
        }
    }
    out
}

// ---------------------------------------------------------------- writing

fn is_valid_name(s: &str) -> bool {
    let mut b = s.bytes();
    matches!(b.next(), Some(c) if c.is_ascii_lowercase())
        && b.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
        && s.len() <= 31
}

fn find_body<'a>(doc: &'a Document, id: &str) -> Result<(Body<'a>, DefType), DefError> {
    for f in collect(doc) {
        match f {
            Found::Node { parent, node, ty } if format!("{parent}/{}", node.name) == id => {
                return Ok((Body::of_node(node), ty))
            }
            Found::Override { node, ty } if format!("&{}", node.target) == id => {
                return Ok((Body::of_ref(node), ty))
            }
            Found::Helper { index, call, .. } if format!("helper:{index}") == id => {
                return Err(DefError::ReadOnly(call.name.clone()))
            }
            _ => {}
        }
    }
    Err(DefError::NotFound(id.to_string()))
}

/// One indentation step as used by the file (from a node's body indent).
fn indent_unit(outer: &str, inner: &str) -> String {
    match inner.strip_prefix(outer) {
        Some(u) if !u.is_empty() => u.to_string(),
        _ if outer.contains('\t') => "\t".into(),
        _ => "    ".into(),
    }
}

/// Indentation for properties inside `body`.
fn child_indent(doc: &Document, body: Body) -> String {
    if let Some(first) = body.children.first() {
        let pos = first.span().start;
        if doc.source()[doc.line_start(pos)..pos].trim().is_empty() {
            return doc.indent_at(pos).to_string();
        }
    }
    let outer = doc.indent_at(body.span.start);
    format!("{outer}{}", indent_unit(outer, ""))
}

fn bad(label: &str, problem: impl Into<String>) -> DefError {
    DefError::BadValue {
        label: label.to_string(),
        problem: problem.into(),
    }
}

fn layer_token(ctx: &Ctx, i: usize) -> Result<String, DefError> {
    match ctx.layer_names.iter().find(|(n, _)| *n == i) {
        Some((_, Some(define))) => Ok(define.clone()),
        Some((_, None)) => Ok(i.to_string()),
        None if ctx.layer_names.is_empty() => Ok(i.to_string()),
        None => Err(DefError::NoSuchLayer(i)),
    }
}

fn check_binding(label: &str, b: &str, bare: bool) -> Result<String, DefError> {
    let b = normalize_ws(b);
    if !b.starts_with('&') || b.len() < 2 {
        return Err(bad(label, format!("needs a behavior like &kp (got “{b}”)")));
    }
    if b.contains(['<', '>', ';', ',', '{', '}', '"']) {
        return Err(bad(label, "can't contain < > ; , { } or quotes"));
    }
    if bare && b.contains(' ') {
        return Err(bad(
            label,
            "takes behaviors without parameters here (e.g. &kp, &mo)",
        ));
    }
    if b[1..].contains('&') {
        return Err(bad(label, "has more than one behavior in one entry"));
    }
    Ok(b)
}

/// Value text (between `=` and `;`) for a new value; `None` = boolean property present.
fn format_value(
    ctx: &Ctx,
    s: &PropSchema,
    v: &NewProp,
    multiline_indent: Option<&str>,
    nl: &str,
) -> Result<Option<String>, DefError> {
    let label = s.label;
    let cells = |items: Vec<String>| format!("<{}>", items.join(" "));
    Ok(Some(match (&s.ty, v) {
        (PropType::Flag, NewProp::Flag(true)) => return Ok(None),
        (PropType::Ms, NewProp::Int(i)) => {
            if !(0..=3_600_000).contains(i) {
                return Err(bad(label, "must be between 0 and 3600000 ms"));
            }
            format!("<{i}>")
        }
        (PropType::Choice { options }, NewProp::Text(t)) => {
            if !options.iter().any(|(o, _)| o == t) {
                return Err(bad(label, format!("can't be “{t}”")));
            }
            format!("\"{t}\"")
        }
        (PropType::Bindings { count, bare }, NewProp::Bindings(list)) => {
            if let Some(n) = count {
                if list.len() != *n {
                    return Err(bad(label, format!("needs exactly {n} entries")));
                }
            }
            if list.is_empty() {
                return Err(bad(label, "needs at least one entry"));
            }
            let items = list
                .iter()
                .map(|b| check_binding(label, b, *bare).map(|b| format!("<{b}>")))
                .collect::<Result<Vec<_>, _>>()?;
            match multiline_indent {
                Some(ind) => items.join(&format!(",{nl}{ind}")),
                None => items.join(", "),
            }
        }
        (PropType::Positions, NewProp::Numbers(ns)) => {
            if ns.is_empty() || ns.iter().any(|n| *n < 0 || *n > 255) {
                return Err(bad(label, "needs at least one key"));
            }
            if s.name == "key-positions" && ns.len() < 2 {
                return Err(bad(label, "needs at least two keys"));
            }
            cells(ns.iter().map(i64::to_string).collect())
        }
        (PropType::Layers, NewProp::Layers(ls)) => {
            if ls.is_empty() {
                return Err(bad(label, "needs at least one layer"));
            }
            cells(
                ls.iter()
                    .map(|l| layer_token(ctx, *l))
                    .collect::<Result<_, _>>()?,
            )
        }
        (PropType::Layer, NewProp::Layers(ls)) if ls.len() == 1 => cells(vec![layer_token(ctx, ls[0])?]),
        (PropType::Mods, NewProp::Names(ns)) => {
            if ns.is_empty() || ns.iter().any(|n| !MODS.contains(&n.as_str())) {
                return Err(bad(label, "needs one or more of MOD_LSFT, MOD_RSFT, MOD_LCTL, …"));
            }
            format!("<({})>", ns.join("|"))
        }
        (PropType::Keys, NewProp::Names(ns)) => {
            if ns.iter().any(|n| catalog::keycode(n).is_none()) {
                return Err(bad(label, "contains an unknown key name"));
            }
            cells(ns.clone())
        }
        _ => return Err(bad(label, "got a value of the wrong kind")),
    }))
}

pub const MODS: [&str; 8] = [
    "MOD_LSFT", "MOD_RSFT", "MOD_LCTL", "MOD_RCTL", "MOD_LALT", "MOD_RALT", "MOD_LGUI", "MOD_RGUI",
];

/// Patch that removes `span` and, if it is alone on its line (apart from a trailing
/// comment), the whole line.
fn remove_line(doc: &Document, span: Span) -> Patch {
    let src = doc.source();
    let start = doc.line_start(span.start);
    let before = &src[start..span.start];
    let rest = &src[span.end..];
    let eol = rest.find('\n').map_or(src.len(), |i| span.end + i + 1);
    let after = src[span.end..eol].trim();
    let comment_only =
        after.is_empty() || after.starts_with("//") || (after.starts_with("/*") && after.ends_with("*/"));
    if before.trim().is_empty() && comment_only {
        Patch::replace(Span::new(start, eol), "")
    } else {
        Patch::replace(span, "")
    }
}

/// Edits that set property `name` of definition `id` to `value`.
pub fn set_prop(
    doc: &Document,
    km: Option<&Keymap>,
    id: &str,
    name: &str,
    value: &NewProp,
) -> Result<Vec<Patch>, DefError> {
    let (body, ty) = find_body(doc, id)?;
    let schema = schema(ty);
    let s = schema
        .props
        .iter()
        .find(|s| s.name == name)
        .ok_or_else(|| bad(name, "isn't a known setting here"))?;
    let existing = body.property(name);
    let remove = matches!(value, NewProp::Remove) || matches!(value, NewProp::Flag(false));
    if remove {
        if s.required {
            return Err(bad(s.label, "is required"));
        }
        return Ok(existing
            .map(|p| vec![remove_line(doc, p.span)])
            .unwrap_or_default());
    }
    let ctx = Ctx::new(doc, km);
    let nl = doc.newline();
    if let Some(p) = existing {
        // Minimal edit for bindings: same count → patch only the changed entries.
        if let (PropType::Bindings { bare, .. }, NewProp::Bindings(new)) = (&s.ty, value) {
            if let Some(spans) = binding_spans(p) {
                if spans.len() == new.len() && !spans.is_empty() {
                    let mut out = Vec::new();
                    for (sp, b) in spans.iter().zip(new) {
                        let b = check_binding(s.label, b, *bare)?;
                        if normalize_ws(doc.text(*sp)) != b {
                            out.push(Patch::replace(*sp, b));
                        }
                    }
                    return Ok(out);
                }
            }
        }
        let multiline = p
            .values_span
            .filter(|v| doc.text(*v).contains('\n'))
            .map(|v| doc.indent_at(v.start).to_string());
        let text = format_value(&ctx, s, value, multiline.as_deref(), nl)?;
        return Ok(vec![match (text, p.values_span) {
            (Some(t), Some(v)) => Patch::replace(v, t),
            (Some(t), None) => Patch::replace(p.span, format!("{name} = {t};")),
            (None, Some(_)) => Patch::replace(p.span, format!("{name};")),
            (None, None) => return Ok(vec![]),
        }]);
    }
    let text = format_value(&ctx, s, value, None, nl)?;
    let line = match text {
        Some(t) => format!("{name} = {t};"),
        None => format!("{name};"),
    };
    Ok(vec![insert_into(doc, body, &line)])
}

/// Insert one line (a property or node text, possibly multi-line already indented
/// relative to the body) just before the body's closing brace.
fn insert_into(doc: &Document, body: Body, text: &str) -> Patch {
    let nl = doc.newline();
    let indent = child_indent(doc, body);
    let close = body.close_brace.start;
    let ls = doc.line_start(close);
    let indented: String = text
        .split('\n')
        .map(|l| {
            if l.is_empty() {
                String::new()
            } else {
                format!("{indent}{l}")
            }
        })
        .collect::<Vec<_>>()
        .join(nl);
    if doc.source()[ls..close].trim().is_empty() {
        Patch::insert(ls, format!("{indented}{nl}"))
    } else {
        Patch::insert(
            close,
            format!("{nl}{indented}{nl}{}", doc.indent_at(body.span.start)),
        )
    }
}

/// Which container a new definition goes into, and its `compatible` if it has one.
fn container_for(t: DefType) -> (&'static str, Option<&'static str>) {
    match t {
        DefType::Macro => ("macros", None),
        DefType::Combo => ("combos", Some("zmk,combos")),
        DefType::ConditionalLayer => ("conditional_layers", Some("zmk,conditional-layers")),
        _ => ("behaviors", None),
    }
}

fn all_labels_and_names(doc: &Document) -> Vec<String> {
    let mut out = Vec::new();
    for i in doc.walk() {
        if let Item::Node(n) = i {
            out.push(n.name.clone());
            out.extend(n.labels.iter().map(|l| l.name.clone()));
        }
    }
    out
}

/// Starting values for a new definition (used where the caller gives none).
fn template(t: DefType) -> Vec<(&'static str, NewProp)> {
    let b = |v: &[&str]| NewProp::Bindings(v.iter().map(|s| s.to_string()).collect());
    match t {
        DefType::HoldTap => vec![
            ("flavor", NewProp::Text("balanced".into())),
            ("tapping-term-ms", NewProp::Int(200)),
            ("bindings", b(&["&kp", "&kp"])),
        ],
        DefType::ModMorph => vec![
            ("bindings", b(&["&kp COMMA", "&kp SEMI"])),
            ("mods", NewProp::Names(vec!["MOD_LSFT".into(), "MOD_RSFT".into()])),
        ],
        DefType::TapDance => vec![
            ("tapping-term-ms", NewProp::Int(200)),
            ("bindings", b(&["&kp A", "&kp B"])),
        ],
        DefType::StickyKey => vec![
            ("release-after-ms", NewProp::Int(1000)),
            ("bindings", b(&["&kp"])),
        ],
        DefType::CapsWord => vec![],
        DefType::Macro => vec![("bindings", b(&["&kp H", "&kp I"]))],
        DefType::Combo => vec![
            ("key-positions", NewProp::Numbers(vec![0, 1])),
            ("bindings", b(&["&kp ESC"])),
        ],
        DefType::ConditionalLayer => vec![
            ("if-layers", NewProp::Layers(vec![1, 2])),
            ("then-layer", NewProp::Layers(vec![3])),
        ],
        DefType::Other => vec![],
    }
}

/// Add a new definition named `name` (also its `&label` for behaviors and macros).
/// `props` override the template's starting values.
pub fn add(
    doc: &Document,
    km: Option<&Keymap>,
    t: DefType,
    name: &str,
    props: &[(String, NewProp)],
) -> Result<Patch, DefError> {
    if t == DefType::Other {
        return Err(DefError::NotTunable("this type".into()));
    }
    if !is_valid_name(name) {
        return Err(DefError::BadName(name.to_string()));
    }
    if all_labels_and_names(doc).iter().any(|n| n == name)
        || catalog::builtin_behaviors()
            .iter()
            .any(|b| b.reference == format!("&{name}"))
    {
        return Err(DefError::NameTaken(name.to_string()));
    }
    let ctx = Ctx::new(doc, km);
    let schema = schema(t);
    let mut values: Vec<(&str, NewProp)> = template(t);
    for (k, v) in props {
        match values.iter_mut().find(|(n, _)| n == k) {
            Some(slot) => slot.1 = v.clone(),
            None => {
                let s = schema
                    .props
                    .iter()
                    .find(|s| s.name == k)
                    .ok_or_else(|| bad(k, "isn't a known setting here"))?;
                values.push((s.name, v.clone()));
            }
        }
    }
    let mut lines: Vec<String> = Vec::new();
    if let Some(c) = schema.compatible {
        lines.push(format!("compatible = \"{c}\";"));
    }
    if let Some(n) = schema.binding_cells {
        lines.push(format!("#binding-cells = <{n}>;"));
    }
    for (k, v) in &values {
        let s = schema
            .props
            .iter()
            .find(|s| s.name == *k)
            .expect("template uses schema names");
        if matches!(v, NewProp::Remove | NewProp::Flag(false)) {
            continue;
        }
        lines.push(match format_value(&ctx, s, v, None, "\n")? {
            Some(text) => format!("{k} = {text};"),
            None => format!("{k};"),
        });
    }
    let head = match t {
        DefType::Combo | DefType::ConditionalLayer => format!("{name} {{"),
        _ => format!("{name}: {name} {{"),
    };
    let node_text = |unit: &str| {
        format!(
            "{head}\n{}\n}};",
            lines
                .iter()
                .map(|l| format!("{unit}{l}"))
                .collect::<Vec<_>>()
                .join("\n")
        )
    };

    let (container, compat) = container_for(t);
    // Existing container?
    let found = doc.walk().find_map(|i| match i {
        Item::Node(n) if n.name == container && (compat.is_none() || n.compatible() == compat) => Some(n),
        _ => None,
    });
    if let Some(c) = found {
        let body = Body::of_node(c);
        let unit = indent_unit(doc.indent_at(c.span.start), &child_indent(doc, body));
        let has_children = c.child_nodes().next().is_some();
        let text = node_text(&unit);
        let text = if has_children { format!("\n{text}") } else { text };
        return Ok(insert_into(doc, body, &text));
    }
    // Create the container inside the root node that holds the keymap, before it.
    let root = doc
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Node(n) if n.name == "/" => Some(n),
            _ => None,
        })
        .find(|n| n.child_nodes().any(|c| c.compatible() == Some("zmk,keymap")))
        .or_else(|| {
            doc.items.iter().find_map(|i| match i {
                Item::Node(n) if n.name == "/" => Some(n),
                _ => None,
            })
        })
        .ok_or_else(|| DefError::NotFound("the root node `/ { … }`".into()))?;
    let root_body = Body::of_node(root);
    let inner = child_indent(doc, root_body);
    let unit = indent_unit(doc.indent_at(root.span.start), &inner);
    let mut container_text = format!("{container} {{\n");
    if let Some(c) = compat {
        container_text += &format!("{unit}compatible = \"{c}\";\n\n");
    }
    for l in node_text(&unit).lines() {
        container_text += &if l.is_empty() {
            "\n".into()
        } else {
            format!("{unit}{l}\n")
        };
    }
    container_text += "};\n";
    let keymap_node = root.child_nodes().find(|c| c.compatible() == Some("zmk,keymap"));
    let nl = doc.newline();
    match keymap_node {
        Some(k) => {
            let indent = doc.indent_at(k.span.start).to_string();
            let text: String = container_text
                .lines()
                .map(|l| {
                    if l.is_empty() {
                        nl.to_string()
                    } else {
                        format!("{indent}{l}{nl}")
                    }
                })
                .collect();
            Ok(Patch::insert(doc.line_start(k.span.start), format!("{text}{nl}")))
        }
        None => Ok(insert_into(doc, root_body, container_text.trim_end())),
    }
}

/// Add `&target { };` to tune a built-in behavior (e.g. `&mt`), or return its id if it
/// already exists.
pub fn tune_builtin(doc: &Document, target: &str) -> Result<(String, Option<Patch>), DefError> {
    if builtin_type(target).is_none() {
        return Err(DefError::NotTunable(format!("&{target}")));
    }
    let id = format!("&{target}");
    if find_body(doc, &id).is_ok() {
        return Ok((id, None));
    }
    let nl = doc.newline();
    let src = doc.source();
    let sep = if src.ends_with(&format!("{nl}{nl}")) || src.is_empty() {
        ""
    } else if src.ends_with('\n') {
        nl
    } else {
        "\n\n"
    };
    Ok((
        id,
        Some(Patch::insert(src.len(), format!("{sep}&{target} {{{nl}}};{nl}"))),
    ))
}

/// Remove a definition. Refused while keys (or other definitions) still use it.
pub fn remove(doc: &Document, id: &str) -> Result<Patch, DefError> {
    let (body, _) = find_body(doc, id)?;
    if let Some(label) = id.strip_prefix('&') {
        return Ok(remove_line_block(doc, body.span, label));
    }
    let node = doc
        .walk()
        .find_map(|i| match i {
            Item::Node(n) if n.span == body.span => Some(n),
            _ => None,
        })
        .ok_or_else(|| DefError::NotFound(id.into()))?;
    if let Some(l) = node.labels.first() {
        let uses = count_uses(doc, &l.name, node.span);
        if uses > 0 {
            return Err(DefError::InUse(l.name.clone(), uses));
        }
    }
    Ok(remove_line_block(doc, node.span, &node.name))
}

/// Remove a multi-line block (whole lines) plus one blank line before it if that leaves
/// two blank lines in a row.
fn remove_line_block(doc: &Document, span: Span, _name: &str) -> Patch {
    let p = remove_line(doc, span);
    let src = doc.source();
    let start = p.span.start;
    // Also drop a blank line directly before the block when another one follows.
    if p.span.start == doc.line_start(span.start) && start > 0 {
        let prev_start = doc.line_start(start - 1);
        let prev = &src[prev_start..start];
        let next_line_blank = src[p.span.end..]
            .lines()
            .next()
            .is_none_or(|l| l.trim().is_empty() || l.trim() == "};");
        if prev.trim().is_empty() && next_line_blank {
            return Patch::replace(Span::new(prev_start, p.span.end), "");
        }
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    const OWNER: &str = include_str!("../tests/fixtures/xnmrph_corne.keymap");
    const ADVANCED: &str = include_str!("../tests/fixtures/advanced.keymap");
    const HELPERS: &str = include_str!("../tests/fixtures/helper_macros.keymap");

    fn load(src: &str) -> (Document, Option<Keymap>) {
        let doc = Document::parse(src);
        let km = Keymap::from_document(&doc).ok();
        (doc, km)
    }

    fn defs(src: &str) -> Vec<Definition> {
        let (doc, km) = load(src);
        list(&doc, km.as_ref())
    }

    fn val<'a>(d: &'a Definition, name: &str) -> Option<&'a PropValue> {
        d.props.iter().find(|p| p.name == name)?.value.as_ref()
    }

    fn edit(src: &str, f: impl FnOnce(&Document, Option<&Keymap>) -> Vec<Patch>) -> String {
        let (doc, km) = load(src);
        let out = doc.apply(&f(&doc, km.as_ref())).unwrap();
        assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
        out.source().to_string()
    }

    #[test]
    fn reads_the_owners_definitions() {
        let d = defs(OWNER);
        let hm = d.iter().find(|d| d.id == "behaviors/homerow_mods").unwrap();
        assert_eq!(hm.def_type, DefType::HoldTap);
        assert_eq!(hm.label.as_deref(), Some("hm"));
        assert_eq!(val(hm, "tapping-term-ms"), Some(&PropValue::Int(200)));
        assert_eq!(val(hm, "flavor"), Some(&PropValue::Text("tap-preferred".into())));
        assert_eq!(
            val(hm, "bindings"),
            Some(&PropValue::Bindings(vec!["&kp".into(), "&kp".into()]))
        );
        assert_eq!(val(hm, "retro-tap"), None);
        let pm = d.iter().find(|d| d.id == "behaviors/paraless").unwrap();
        assert_eq!(val(pm, "mods"), Some(&PropValue::Names(vec!["MOD_LSFT".into()])));
        let caps = d.iter().find(|d| d.id == "behaviors/caps").unwrap();
        assert_eq!(
            val(caps, "continue-list"),
            Some(&PropValue::Names(vec!["MINUS".into(), "BACKSPACE".into()]))
        );
        let tab = d.iter().find(|d| d.id == "macros/browsertab").unwrap();
        assert_eq!(tab.uses, 1);
        assert_eq!(
            val(tab, "bindings"),
            Some(&PropValue::Bindings(
                [
                    "&macro_press",
                    "&kp LCTRL",
                    "&macro_tap",
                    "&kp TAB",
                    "&macro_release",
                    "&kp LCTRL"
                ]
                .map(String::from)
                .to_vec()
            ))
        );
        let ae = d.iter().find(|d| d.id == "macros/ae").unwrap();
        assert_eq!(ae.title, "AE");
        let PropValue::Bindings(steps) = val(ae, "bindings").unwrap() else {
            panic!()
        };
        assert_eq!(steps.len(), 9, "a <…> group with four keys is four steps");
        let combos: Vec<_> = d.iter().filter(|d| d.def_type == DefType::Combo).collect();
        assert_eq!(combos.len(), 19);
        assert_eq!(
            val(combos[0], "key-positions"),
            Some(&PropValue::Numbers(vec![13, 29]))
        );
        let perc = d.iter().find(|d| d.id == "combos/perc").unwrap();
        assert_eq!(val(perc, "layers"), Some(&PropValue::Layers(vec![0])));
        let cl = d
            .iter()
            .find(|d| d.def_type == DefType::ConditionalLayer)
            .unwrap();
        assert_eq!(val(cl, "if-layers"), Some(&PropValue::Layers(vec![4, 2])));
        assert_eq!(val(cl, "then-layer"), Some(&PropValue::Layers(vec![5])));
    }

    #[test]
    fn defines_resolve_and_helpers_are_read_only() {
        let d = defs(ADVANCED);
        let tri = d
            .iter()
            .find(|d| d.def_type == DefType::ConditionalLayer)
            .unwrap();
        assert_eq!(val(tri, "if-layers"), Some(&PropValue::Layers(vec![1, 2])));
        let h = defs(HELPERS);
        let hml = h.iter().find(|d| d.node_name == "hml").unwrap();
        assert_eq!(hml.source, Source::Helper);
        assert!(hml.read_only.is_some());
        assert_eq!(val(hml, "tapping-term-ms"), Some(&PropValue::Int(280)));
        assert_eq!(val(hml, "flavor"), Some(&PropValue::Text("balanced".into())));
        let esc = h.iter().find(|d| d.node_name == "esc").unwrap();
        assert_eq!(val(esc, "key-positions"), Some(&PropValue::Numbers(vec![0, 1])));
        assert_eq!(
            val(esc, "bindings"),
            Some(&PropValue::Bindings(vec!["&kp ESC".into()]))
        );
        let mt = h.iter().find(|d| d.id == "&mt").unwrap();
        assert_eq!(mt.source, Source::Override);
        let (doc, km) = load(HELPERS);
        assert!(matches!(
            set_prop(
                &doc,
                km.as_ref(),
                "helper:0",
                "flavor",
                &NewProp::Text("balanced".into())
            ),
            Err(DefError::ReadOnly(_))
        ));
    }

    #[test]
    fn property_edits_are_minimal() {
        let out = edit(OWNER, |d, k| {
            set_prop(
                d,
                k,
                "behaviors/homerow_mods",
                "tapping-term-ms",
                &NewProp::Int(230),
            )
            .unwrap()
        });
        assert_eq!(
            out,
            OWNER.replacen("tapping-term-ms = <200>;", "tapping-term-ms = <230>;", 1)
        );
        // New property goes in before the closing brace, with the body's indentation.
        let out = edit(OWNER, |d, k| {
            set_prop(d, k, "behaviors/homerow_mods", "retro-tap", &NewProp::Flag(true)).unwrap()
        });
        assert!(
            out.contains(
                "require-prior-idle-ms = <120>; /* tune to taste */\n            retro-tap;\n        };"
            ),
            "{}",
            &out[..900]
        );
        // Removing a property removes its line (with its trailing comment).
        let out = edit(OWNER, |d, k| {
            set_prop(
                d,
                k,
                "behaviors/homerow_mods",
                "require-prior-idle-ms",
                &NewProp::Remove,
            )
            .unwrap()
        });
        assert_eq!(
            out,
            OWNER.replacen(
                "            require-prior-idle-ms = <120>; /* tune to taste */\n",
                "",
                1
            )
        );
        // Changing one macro step patches only that step.
        let mut steps: Vec<String> = [
            "&macro_press",
            "&kp LCTRL",
            "&macro_tap",
            "&kp TAB",
            "&macro_release",
            "&kp LCTRL",
        ]
        .map(String::from)
        .to_vec();
        steps[3] = "&kp Q".into();
        let out = edit(OWNER, |d, k| {
            set_prop(
                d,
                k,
                "macros/browsertab",
                "bindings",
                &NewProp::Bindings(steps.clone()),
            )
            .unwrap()
        });
        assert_eq!(out, OWNER.replacen("<&kp TAB>", "<&kp Q>", 1));
        // Adding a step rewrites the list in the file's multi-line style.
        steps.push("&kp ENTER".into());
        let out = edit(OWNER, |d, k| {
            set_prop(
                d,
                k,
                "macros/browsertab",
                "bindings",
                &NewProp::Bindings(steps.clone()),
            )
            .unwrap()
        });
        assert!(
            out.contains("<&macro_release>,\n                <&kp LCTRL>,\n                <&kp ENTER>;"),
            "{}",
            &out[1500..2300]
        );
        // Combo key positions and layers (layers written with the file's #defines).
        let out = edit(OWNER, |d, k| {
            set_prop(d, k, "combos/perc", "layers", &NewProp::Layers(vec![0, 2])).unwrap()
        });
        assert!(out.contains("key-positions = <5 17>;\n            layers = <DEF SYM>;"));
        let out = edit(OWNER, |d, k| {
            set_prop(
                d,
                k,
                "behaviors/paraless",
                "mods",
                &NewProp::Names(vec!["MOD_LSFT".into(), "MOD_RSFT".into()]),
            )
            .unwrap()
        });
        assert!(out.contains("mods = <(MOD_LSFT|MOD_RSFT)>;"));
    }

    #[test]
    fn invalid_values_are_refused() {
        let (doc, km) = load(OWNER);
        let k = km.as_ref();
        let e = |id: &str, n: &str, v: NewProp| set_prop(&doc, k, id, n, &v).unwrap_err();
        assert!(matches!(
            e("behaviors/homerow_mods", "flavor", NewProp::Text("fast".into())),
            DefError::BadValue { .. }
        ));
        assert!(matches!(
            e(
                "behaviors/homerow_mods",
                "bindings",
                NewProp::Bindings(vec!["&kp A".into(), "&kp".into()])
            ),
            DefError::BadValue { .. }
        ));
        assert!(matches!(
            e("behaviors/homerow_mods", "bindings", NewProp::Remove),
            DefError::BadValue { .. }
        ));
        assert!(matches!(
            e("combos/perc", "key-positions", NewProp::Numbers(vec![3])),
            DefError::BadValue { .. }
        ));
        assert!(matches!(
            e("combos/perc", "layers", NewProp::Layers(vec![9])),
            DefError::NoSuchLayer(9)
        ));
        assert!(matches!(
            e(
                "behaviors/caps",
                "continue-list",
                NewProp::Names(vec!["NOPE".into()])
            ),
            DefError::BadValue { .. }
        ));
        assert!(matches!(
            e("combos/nope", "layers", NewProp::Remove),
            DefError::NotFound(_)
        ));
        assert!(matches!(
            e(
                "macros/ae",
                "bindings",
                NewProp::Bindings(vec!["&kp A; evil".into()])
            ),
            DefError::BadValue { .. }
        ));
    }

    #[test]
    fn add_and_remove_definitions() {
        // A new combo goes into the existing container.
        let out = edit(OWNER, |d, k| {
            vec![add(
                d,
                k,
                DefType::Combo,
                "copy_c",
                &[
                    ("key-positions".into(), NewProp::Numbers(vec![26, 27])),
                    ("bindings".into(), NewProp::Bindings(vec!["&kp LC(C)".into()])),
                ],
            )
            .unwrap()]
        });
        assert!(out.contains("\n\n        copy_c {\n            key-positions = <26 27>;\n            bindings = <&kp LC(C)>;\n        };\n    };"));
        let d = defs(&out);
        assert_eq!(d.iter().filter(|d| d.def_type == DefType::Combo).count(), 20);
        // Removing it gives back the original file.
        let back = edit(&out, |d, _| vec![remove(d, "combos/copy_c").unwrap()]);
        assert_eq!(back, OWNER);

        // A new hold-tap is usable as &hrm and appears in the key picker catalog.
        let out = edit(OWNER, |d, k| {
            vec![add(d, k, DefType::HoldTap, "hrm", &[]).unwrap()]
        });
        let (doc, km) = load(&out);
        assert!(catalog::custom_behaviors(&doc, km.as_ref().unwrap())
            .iter()
            .any(|b| b.reference == "&hrm"));
        assert_eq!(
            edit(&out, |d, _| vec![remove(d, "behaviors/hrm").unwrap()]),
            OWNER
        );

        // Used definitions can't be removed; names are checked.
        let (doc, km) = load(OWNER);
        assert_eq!(
            remove(&doc, "macros/browsertab"),
            Err(DefError::InUse("browsertab".into(), 1))
        );
        assert_eq!(
            add(&doc, km.as_ref(), DefType::Macro, "browsertab", &[]),
            Err(DefError::NameTaken("browsertab".into()))
        );
        assert_eq!(
            add(&doc, km.as_ref(), DefType::Macro, "kp", &[]),
            Err(DefError::NameTaken("kp".into()))
        );
        assert_eq!(
            add(&doc, km.as_ref(), DefType::Macro, "Bad Name", &[]),
            Err(DefError::BadName("Bad Name".into()))
        );
    }

    #[test]
    fn containers_are_created_when_missing() {
        let stock = include_str!("../tests/fixtures/corne_stock.keymap");
        let out = edit(stock, |d, k| vec![add(d, k, DefType::Combo, "esc", &[]).unwrap()]);
        let (a, b, c) = (" ".repeat(8), " ".repeat(16), " ".repeat(24));
        let want = format!("/ {{\n{a}combos {{\n{b}compatible = \"zmk,combos\";\n\n{b}esc {{\n{c}key-positions = <0 1>;\n{c}bindings = <&kp ESC>;\n{b}}};\n{a}}};\n\n{a}keymap {{");
        assert!(out.contains(&want), "{out}");
        let out = edit(&out, |d, k| {
            vec![add(d, k, DefType::TapDance, "td_q", &[]).unwrap()]
        });
        let d = defs(&out);
        assert!(d
            .iter()
            .any(|d| d.id == "behaviors/td_q" && d.def_type == DefType::TapDance));
        assert!(d.iter().any(|d| d.id == "combos/esc"));
    }

    #[test]
    fn tuning_builtins() {
        let (doc, _) = load(OWNER);
        let (id, p) = tune_builtin(&doc, "mt").unwrap();
        assert_eq!(id, "&mt");
        let out = doc.apply(&[p.unwrap()]).unwrap();
        let km = Keymap::from_document(&out).ok();
        let p = set_prop(&out, km.as_ref(), "&mt", "tapping-term-ms", &NewProp::Int(250)).unwrap();
        let out = out.apply(&p).unwrap();
        assert!(
            out.source()
                .ends_with("};\n\n&mt {\n    tapping-term-ms = <250>;\n};\n"),
            "{:?}",
            &out.source()[out.source().len() - 80..]
        );
        let d = list(&out, None);
        assert_eq!(
            val(d.iter().find(|d| d.id == "&mt").unwrap(), "tapping-term-ms"),
            Some(&PropValue::Int(250))
        );
        assert!(tune_builtin(&doc, "kp").is_err());
    }
}
