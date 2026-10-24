import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

// Mirrors the Rust types in src-tauri/src/lib.rs and src-tauri/src/rig.rs
// (and patchpilot_parity::Inventory). Keep them in sync.

export interface AppVersion {
  app: string;
  manifest_schema: number;
}

export type PluginFormat = "vst2" | "vst3" | "au" | "clap" | "aax" | "jsfx";
export type Confidence = "low" | "medium" | "high";
export type Os = "windows" | "macos";

export interface ProductRow {
  id: string;
  name: string;
  formats: PluginFormat[];
  version: string | null;
  confidence: Confidence;
}

export interface VendorGroup {
  /** Empty when no vendor could be read. */
  vendor: string;
  products: ProductRow[];
}

export interface AppRow {
  name: string;
  kind: "vendor_app" | "license_tool";
  version: string | null;
}

export interface Inventory {
  plugins: number;
  products: number;
  formats: { format: PluginFormat; count: number }[];
  vendors: VendorGroup[];
  apps: AppRow[];
  templates: number;
}

export interface Overview {
  machine: { id: string; name: string; os: Os; scanned_at: number } | null;
  inventory: Inventory | null;
  golden: { version: number; created_at: number; from_this_machine: boolean } | null;
}

export interface ScanReport {
  overview: Overview;
  unchanged: boolean;
  warnings: string[];
}

export interface Progress {
  step: number;
  of: number;
  label: string;
}

export const appVersion = () => invoke<AppVersion>("app_version");
export const overview = () => invoke<Overview>("overview");
export const scanMachine = (name: string | null) =>
  invoke<ScanReport>("scan_machine", { name });
export const makeGolden = () => invoke<Overview>("make_golden");

export function onScanProgress(f: (p: Progress) => void): Promise<UnlistenFn> {
  return listen<Progress>("scan-progress", (e) => f(e.payload));
}

export const FORMAT_LABEL: Record<PluginFormat, string> = {
  vst3: "VST3",
  au: "AU",
  clap: "CLAP",
  aax: "AAX",
  vst2: "VST2",
  jsfx: "JSFX",
};

// Comparison view (#41): src-tauri/src/compare.rs and patchpilot_parity::Comparison.

export type ParityState =
  | "missing"
  | "incomplete"
  | "outdated"
  | "modified"
  | "version_unknown"
  | "newer_than_golden"
  | "extra"
  | "not_applicable"
  | "in_parity";

export type ItemKind = "plugin" | "daw_asset" | "vendor_app" | "license_tool" | "folder" | "config";

export interface MachineCard {
  id: string;
  name: string;
  os: Os;
  scanned_at: number;
  is_this_machine: boolean;
  profile: { id: string; name: string } | null;
  score: number | null;
}

export interface MachinesView {
  golden_version: number | null;
  machines: MachineCard[];
}

export interface ItemSide {
  format: PluginFormat | null;
  platform: Os;
  version: string | null;
}

export interface ComparisonRow {
  product_id: string;
  name: string;
  vendor: string;
  kind: ItemKind;
  state: ParityState;
  golden_version: string | null;
  machine_version: string | null;
  missing_formats: PluginFormat[];
  confidence: Confidence;
  needed_by_templates: boolean;
  golden_items: ItemSide[];
  machine_items: ItemSide[];
}

export interface Reason {
  text: string;
  products: string[];
  points: number;
}

export interface TemplateReadiness {
  product_id: string;
  name: string;
  missing: string[];
}

export interface Comparison {
  score: number;
  reasons: Reason[];
  rows: ComparisonRow[];
  templates: TemplateReadiness[];
  unresolved: number;
}

export interface MachineComparison {
  machine: MachineCard;
  golden_version: number;
  comparison: Comparison;
}

export type Phase = "foundation" | "vendor_handoff" | "manual";

export interface Step {
  number: number;
  phase: Phase;
  title: string;
  details: string[];
  products: string[];
  after: number[];
}

export const machines = () => invoke<MachinesView>("machines");
export const compareMachine = (id: string) => invoke<MachineComparison>("compare_machine", { id });
export const planMachine = (id: string) => invoke<{ steps: Step[] }>("plan_machine", { id });
export const importBundle = (path: string) => invoke<{ lines: string[] }>("import_bundle", { path });

// Bring over (decision #44); mirrors src-tauri/src/bring.rs and patchpilot_local::bring.

export interface FileToBring {
  /** Group heading: "Reaper". */
  app: string;
  /** "Track template", "FX chain", "Script", "Theme", ... */
  kind: string;
  root: string;
  /** `/`-separated path below the root. */
  rel: string;
  hash: string;
  size: number | null;
  /** This machine has a different copy, backed up before it is replaced. */
  replaces: boolean;
  too_large: boolean;
}

export interface BringView {
  linked: boolean;
  files: FileToBring[];
}

export interface BringResult {
  written: number;
  backed_up: number;
  backup_dir: string | null;
  unavailable: string[];
}

export const fileKey = (f: FileToBring) => `${f.root}/${f.rel}`;
export const filesToBring = () => invoke<BringView>("files_to_bring");
export const bringFiles = (keys: string[]) => invoke<BringResult>("bring_files", { keys });

export const STATE_LABEL: Record<ParityState, string> = {
  missing: "Missing",
  incomplete: "Incomplete",
  outdated: "Outdated",
  modified: "Modified",
  version_unknown: "Version unclear",
  newer_than_golden: "Newer than Golden",
  extra: "Not in Golden",
  not_applicable: "Not for this OS",
  in_parity: "In parity",
};

export const STATE_HINT: Record<ParityState, string> = {
  missing: "In the Golden Rig but not on this machine.",
  incomplete: "Installed, but some formats the Golden Rig has are missing.",
  outdated: "Older than the Golden Rig's version.",
  modified: "Same version, but the file's content differs from Golden.",
  version_unknown: "The versions differ in a way we can't order.",
  newer_than_golden: "Newer than the Golden Rig. Consider updating Golden.",
  extra: "On this machine but not in the Golden Rig. Doesn't affect the score.",
  not_applicable: "Golden's formats of this product don't exist on this OS.",
  in_parity: "Matches the Golden Rig.",
};

export const PHASE_LABEL: Record<Phase, string> = {
  foundation: "Foundation",
  vendor_handoff: "Vendor apps",
  manual: "Manual steps",
};

/** Shows the main window (from the tray popover), optionally on one machine's comparison or plan. */
export const openMainWindow = (machine: string | null = null, plan = false) =>
  invoke<void>("open_main_window", { machine, plan });

// Live sync (src-tauri/src/sync.rs, decision #38).

export interface SyncStatus {
  linked: boolean;
  online: boolean;
  error: string | null;
}

export const syncStatus = () => invoke<SyncStatus>("sync_status");
/** Shows a code for another machine to type. `link-finished` fires when it has joined. */
export const linkStart = () => invoke<string>("link_start");
export const linkJoin = (code: string) => invoke<void>("link_join", { code });
export const unlink = () => invoke<void>("unlink");
export const onSyncStatus = (f: (s: SyncStatus) => void): Promise<UnlistenFn> =>
  listen<SyncStatus>("sync-status", (e) => f(e.payload));
export const onLinkFinished = (f: (r: { ok: boolean; error: string | null }) => void): Promise<UnlistenFn> =>
  listen<{ ok: boolean; error: string | null }>("link-finished", (e) => f(e.payload));

// Rig Report (src-tauri/src/report.rs and patchpilot_parity::ReportOptions).

export interface ReportOptions {
  title: string;
  date: string;
  plugins: boolean;
  versions: boolean;
  apps: boolean;
  parity: boolean;
}

export const rigReportPreview = (options: ReportOptions) =>
  invoke<{ html: string; markdown: string }>("rig_report_preview", { options });
/** Markdown when `path` ends in .md, otherwise the web page. */
export const saveRigReport = (path: string, options: ReportOptions) =>
  invoke<void>("save_rig_report", { path, options });
