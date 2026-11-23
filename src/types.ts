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
