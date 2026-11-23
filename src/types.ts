// Mirrors src-tauri/src/summary.rs (camelCase via serde).

export interface LayerSummary {
  index: number;
  title: string;
  nodeName: string;
  bindings: string[];
  readOnlyReason: string | null;
}

export interface KeymapSummary {
  path: string;
  fileName: string;
  layers: LayerSummary[];
  behaviors: number;
  macros: number;
  combos: number;
  conditionalLayers: number;
  definitions: Definition[];
  parseProblems: string[];
}

export interface TargetSummary {
  artifactName: string;
  board: string;
  shields: string[];
  snippets: string[];
  studio: boolean;
  niceView: boolean;
  settingsReset: boolean;
}

export interface BuildSummary {
  targets: TargetSummary[];
  keyboardShields: string[];
  boards: string[];
}

export type RevisionKind = "commit" | "version" | "branch";

export interface ModuleSummary { name: string; url: string | null; revision: string }

export interface FirmwareSummary {
  zmkRevision: string | null;
  zmkRevisionKind: RevisionKind | null;
  zmkUrl: string | null;
  extraModules: ModuleSummary[];
}

export type SettingState = "active" | "commented_out" | "not_set";

export interface SettingSummary { name: string; value: string; state: SettingState; line: number }

export interface ConfSummary { path: string; fileName: string; settings: SettingSummary[]; duplicates: string[] }

export interface RepoSummary {
  root: string;
  keymaps: KeymapSummary[];
  confFiles: ConfSummary[];
  hasWestYml: boolean;
  firmware: FirmwareSummary | null;
  build: BuildSummary | null;
  warnings: string[];
}

// mxkeys-git types
export interface RepoUrl { url: string; kind: "http" | "ssh" | "git" | "local"; dir_name: string }

export type GitBackend = { kind: "system_git"; path: string; version: string } | { kind: "libgit2" };

export type CloneProgress =
  | { stage: "connecting" }
  | { stage: "receiving"; percent: number }
  | { stage: "resolving"; percent: number }
  | { stage: "checking_out"; percent: number }
  | { stage: "done" };

// ----- hardware (mxkeys-hardware / src-tauri detection.rs) -----
export type KeyboardId = "corne";
export type ControllerId = "nice_nano_v2" | "nice_nano_v1" | "xiao_ble" | "nrf_micro";
export type DisplayKind = "none" | "nice_view" | "oled128x32";
export type Side = "left" | "right";
export type Confidence = "weak" | "medium" | "strong";
export type Source = "settings" | "build_yaml" | "quiz" | "usb" | "bootloader" | "live_connection";
export type Known<T> = { type: "known"; id: T } | { type: "other"; id: string };

export interface Evidence {
  source: Source;
  fact: { field: string; value: unknown };
  confidence: Confidence;
  because: string;
}

export interface FieldResult<T> {
  value: T | null;
  confidence: Confidence | null;
  sources: Source[];
  evidence: Evidence[];
  conflicts: string[];
}

export interface HardwareProfile {
  keyboard: KeyboardId;
  controller: ControllerId;
  display: DisplayKind;
  central: Side;
  studio: boolean;
}

export interface Note { level: "info" | "warning"; text: string }

export interface Detection {
  keyboard: FieldResult<Known<KeyboardId>>;
  controller: FieldResult<Known<ControllerId>>;
  display: FieldResult<DisplayKind>;
  central: FieldResult<Side>;
  studio: FieldResult<boolean>;
  notes: Note[];
  summary: string;
  profile: HardwareProfile | null;
  ask: ("keyboard" | "controller" | "display")[];
}

export interface Uf2Info { bootloader: string | null; bootloader_version: string | null; model: string | null; board_id: string | null }
export interface Uf2Drive { path: string; info: Uf2Info }
export interface ProfileTarget { id: "left" | "right" | "settings_reset"; board: string; shields: string[]; snippets: string[] }

export interface DetectionReport {
  detection: Detection;
  drives: Uf2Drive[];
  probeErrors: string[];
  targets: ProfileTarget[];
  unsupported: string[];
}

export interface QuizOption { value: string; label: string; looks_like: string; supported: boolean }
export interface QuizQuestion { id: "controller" | "display"; prompt: string; options: QuizOption[] }
export interface QuizAnswers { keyboard?: KeyboardId | null; controller?: ControllerId | null; display?: DisplayKind | null }

export interface KeyGeom { w: number; h: number; x: number; y: number; r: number; rx: number; ry: number }

// ----- toolchain + builds (src-tauri building.rs, crates/toolchain) -----
export interface ToolchainStatus { host: string | null; root: string; done: string[]; missing: string[]; ready: boolean; size: number }
export interface ToolchainInfo { status: ToolchainStatus; sdkVersion: string; running: boolean }

export type InstallProgress =
  | { kind: "step"; component: string; step: "download" | "unpack" | "trim" | "setup" }
  | { kind: "bytes"; component: string; done: number; total: number | null }
  | { kind: "done"; component: string; freed: number }
  | { kind: "line"; text: string };

export type ToolchainEvent =
  | { type: "progress"; data: InstallProgress }
  | { type: "finished"; data: { ok: boolean; error: string | null } };

export type TargetId = "left" | "right" | "settings_reset";

export interface ProblemLocation { file: string; line: number; layer: string | null; key: number | null; binding: string | null }
export interface Problem { title: string; hint: string | null; location: ProblemLocation | null; raw: string }

export type PipelineEvent =
  | { event: "stage"; text: string }
  | ({ event: "install" } & InstallProgress)
  | { event: "line"; target: TargetId | null; text: string }
  | { event: "target_started"; target: TargetId }
  | { event: "target_finished"; target: TargetId; ok: boolean; uf2: string | null; problems: Problem[] };

export interface TargetResult { target: TargetId; uf2: string | null; problems: Problem[] }
export interface BuildOutcome { profile: HardwareProfile; results: TargetResult[]; firmware_dir: string; lock: string }

export type BuildEvent =
  | { type: "event"; data: PipelineEvent }
  | { type: "finished"; data: { outcome: BuildOutcome | null; error: string | null } };

// ----- flashing (crates/flash session.rs, src-tauri flashing.rs) -----
export type FlashPhase =
  | { phase: "waiting_for_drive" }
  | { phase: "too_many_drives" }
  | { phase: "refused"; reason: string; drive: string }
  | { phase: "copying"; drive: string }
  | { phase: "restarting"; drive: string; polls: number }
  | { phase: "failed"; reason: string }
  | { phase: "finished" };

export interface FlashStepView { title: string; state: "pending" | "current" | "done" | "skipped" }
export interface FlashView { steps: FlashStepView[]; current: number | null; phase: FlashPhase; instruction: string; detail: string | null }
export interface FlashEvent { view: FlashView; progress: [number, number] | null; ended: boolean }

// ----- keymap editor (zmk-config catalog, src-tauri editor.rs) -----
export interface KeycodeInfo {
  names: string[];
  deprecated: string[];
  description: string;
  page: string;
  usage: string;
  shifted: boolean;
  category: string;
  label: string;
}

export interface CommandOption { value: string; label: string; number: [number, number, string] | null }

export type ParamKind =
  | { type: "keycode" }
  | { type: "modifier" }
  | { type: "layer" }
  | { type: "command"; options: CommandOption[] }
  | { type: "free" };

export interface BehaviorParam { name: string; kind: ParamKind }
export interface BehaviorInfo { reference: string; name: string; description: string; category: string; params: BehaviorParam[] }
export interface LayerRef { index: number; title: string; define: string | null }

export interface EditorCatalog { keycodes: KeycodeInfo[]; keycodesSource: string; behaviors: BehaviorInfo[]; macroControls: BehaviorInfo[]; layers: LayerRef[]; schemas: DefSchema[] }

export interface EditResult { keymap: KeymapSummary; version: string; canUndo: boolean; canRedo: boolean }

export type EditOp =
  | { op: "set_binding"; layer: number; position: number; binding: string }
  | { op: "rename_layer"; layer: number; name: string }
  | { op: "add_layer"; name: string }
  | { op: "remove_layer"; layer: number }
  | { op: "move_layer"; from: number; to: number }
  | { op: "set_def_prop"; id: string; prop: string; value: NewProp }
  | { op: "add_def"; def_type: DefType; name: string; props?: [string, NewProp][] }
  | { op: "remove_def"; id: string }
  | { op: "tune_builtin"; target: string };

// ---- Behaviors, macros, combos (Phase 7) ----
export type DefType = "hold_tap" | "mod_morph" | "tap_dance" | "sticky_key" | "caps_word" | "macro" | "combo" | "conditional_layer" | "other";
export type PropTypeInfo =
  | { type: "ms" } | { type: "flag" } | { type: "mods" } | { type: "positions" } | { type: "layers" } | { type: "layer" } | { type: "keys" }
  | { type: "choice"; options: [string, string][] }
  | { type: "bindings"; count: number | null; bare: boolean };
export type PropSchema = PropTypeInfo & { name: string; label: string; help: string; required: boolean; default: string | null };
export interface DefSchema { def_type: DefType; compatible: string | null; title: string; help: string; binding_cells: number | null; props: PropSchema[] }
export type PropValue =
  | { type: "int"; value: number } | { type: "flag"; value: boolean } | { type: "text"; value: string }
  | { type: "bindings"; value: string[] } | { type: "numbers"; value: number[] } | { type: "layers"; value: number[] }
  | { type: "names"; value: string[] } | { type: "raw"; value: string };
export type NewProp = Exclude<PropValue, { type: "raw" }> | { type: "remove" };
export interface PropView { name: string; value: PropValue | null; raw: string | null; line: number | null }
export interface Definition {
  id: string; def_type: DefType; source: "node" | "override" | "helper"; label: string | null; node_name: string; title: string;
  compatible: string | null; props: PropView[]; other_props: [string, string][]; uses: number; read_only: string | null; line: number;
}

// ---- Settings page (Phase 6) ----
export type SettingGroup = "power" | "bluetooth" | "screen" | "lights" | "typing" | "live_editing";
export type Unit = "seconds" | "minutes" | "milliseconds" | "percent";
export interface ChoiceOption { option: string | null; value: number | null; label: string }
export type SettingKind =
  | { type: "toggle"; option: string; default: boolean; also: string[] }
  | { type: "number"; option: string; default: number; min: number; max: number; unit: Unit }
  | { type: "text"; option: string; default: string; max_bytes: number }
  | { type: "choice"; options: ChoiceOption[]; default: number }
  | { type: "pick"; option: string; options: ChoiceOption[]; default: number };
export interface SettingDef { id: string; group: SettingGroup; label: string; help: string; kind: SettingKind; requires: string | null }
export type SettingValue =
  | { type: "bool"; value: boolean }
  | { type: "int"; value: number }
  | { type: "text"; value: string }
  | { type: "choice"; value: number };
export type NewValue = SettingValue | { type: "default" };
export interface SettingView { def: SettingDef; value: SettingValue; isDefault: boolean; lines: number[]; applies: boolean; problem: string | null }
export interface SettingsPage {
  path: string; fileName: string;
  groups: { id: SettingGroup; title: string; settings: SettingView[] }[];
  other: SettingSummary[]; all: SettingSummary[]; duplicates: string[];
  version: string; canUndo: boolean; canRedo: boolean;
}
export type SettingsOp =
  | { op: "set"; id: string; value: NewValue }
  | { op: "set_raw"; name: string; value: string }
  | { op: "remove_raw"; name: string };
export type FirmwareState = "not_built" | "up_to_date" | "outdated";
