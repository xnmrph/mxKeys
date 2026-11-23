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
