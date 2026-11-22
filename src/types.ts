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

export interface RepoSummary {
  root: string;
  keymaps: KeymapSummary[];
  confFiles: string[];
  hasWestYml: boolean;
  build: BuildSummary | null;
  warnings: string[];
}
