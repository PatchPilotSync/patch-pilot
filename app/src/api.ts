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
  presets: { name: string; vendor: string; size: number | null }[];
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
  /** Products here the Golden Rig lacks or has older (not ignored). */
  suggestions: number;
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
  /** Product IDs of rows that could go into the Golden Rig, not ignored. */
  suggested: string[];
}

export type Phase = "foundation" | "vendor_handoff" | "manual";

export interface Step {
  number: number;
  phase: Phase;
  title: string;
  details: string[];
  products: string[];
  after: number[];
  /** Vendor app this step uses (e.g. "native-access"), so it can be opened. */
  app: string | null;
}

export const machines = () => invoke<MachinesView>("machines");
export const compareMachine = (id: string) => invoke<MachineComparison>("compare_machine", { id });
export const planMachine = (id: string) => invoke<{ steps: Step[] }>("plan_machine", { id });
/** "Propose to Golden": a new Golden version taking these products from the machine. Returns the version. */
export const addToGolden = (machine: string, products: string[]) =>
  invoke<number>("add_to_golden", { machine, products });
/** Stop suggesting this version of the product, on every linked machine. */
export const ignoreSuggestion = (product: string, version: string | null) =>
  invoke<void>("ignore_suggestion", { product, version });
export const importBundle = (path: string) => invoke<{ lines: string[] }>("import_bundle", { path });

// Bring over (decision #47); mirrors src-tauri/src/bring.rs and patchpilot_local::bring.

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
  /** Replaced files, saved first so History can undo the bring-over. */
  backed_up: number;
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

// Update alerts (decision #47): a newer version on another machine, in the
// Golden Rig, or in the plug-in catalog.
export type UpdateSource =
  | { kind: "machine"; name: string }
  | { kind: "golden"; version: number }
  | { kind: "catalog"; released: string | null };

export interface UpdateAlert {
  id: string;
  vendor: string;
  name: string;
  formats: PluginFormat[];
  installed: string;
  latest: string;
  source: UpdateSource;
  website: string | null;
}

export interface UpdatesView {
  alerts: UpdateAlert[];
  catalog_updated: string;
}

export const updates = () => invoke<UpdatesView>("updates");

export function sourceLabel(s: UpdateSource): string {
  switch (s.kind) {
    case "machine":
      return `on ${s.name}`;
    case "golden":
      return `in Golden Rig v${s.version}`;
    case "catalog":
      return s.released ? `released ${s.released}` : "latest release";
  }
}


// Installer Vault and vendor apps (src-tauri/src/vault.rs).

export type Arch = "x86_64" | "aarch64" | "universal";

export interface Product {
  canonical_id: string;
  vendor: string;
  name: string;
}

export interface VaultEntry {
  id: string;
  hash: string;
  size: number;
  file: string;
  vendor: string;
  product: string;
  product_id?: string;
  version?: string;
  os?: Os;
  arch?: Arch;
  source_name: string;
  signer?: string;
  added_at: number;
  fits_here: boolean;
  missing: boolean;
}

export interface InboxItem {
  path: string;
  file_name: string;
  size: number;
  modified: number;
  guess: { name: string; version: string | null; os: Os | null; arch: Arch | null };
  product: Product | null;
}

export interface VaultView {
  root: string | null;
  suggested_root: string | null;
  downloads: string | null;
  entries: VaultEntry[];
  inbox: InboxItem[];
  products: Product[];
  problem: string | null;
}

export interface NewEntry {
  vendor: string;
  product: string;
  product_id: string | null;
  version: string | null;
  os: Os | null;
  arch: Arch | null;
}

export interface VaultMatch {
  product_id: string;
  entry_id: string;
  file_name: string;
  version: string | null;
}

export interface VendorApp {
  id: string;
  name: string;
  installed: boolean;
  can_open: boolean;
  download_url: string;
}

export const vaultStatus = () => invoke<VaultView>("vault_status");
export const vaultSetRoot = (path: string) => invoke<VaultView>("vault_set_root", { path });
export const vaultAdd = (path: string, meta: NewEntry) =>
  invoke<{ entry: VaultEntry; already: boolean; signer_warning: string | null }>("vault_add", { path, meta });
export const vaultInspect = (paths: string[]) => invoke<InboxItem[]>("vault_inspect", { paths });
export const vaultDismiss = (path: string) => invoke<void>("vault_dismiss", { path });
export const vaultMatches = (products: string[]) => invoke<VaultMatch[]>("vault_matches", { products });
export const vaultRun = (entryId: string) => invoke<void>("vault_run", { entryId });
export const vaultReveal = (entryId: string | null = null) => invoke<void>("vault_reveal", { entryId });
export const vendorApps = () => invoke<VendorApp[]>("vendor_apps");
export const openVendorApp = (id: string) => invoke<"app" | "download_page">("open_vendor_app", { id });
export const onVaultChanged = (f: () => void): Promise<UnlistenFn> => listen("vault-changed", () => f());

// New-machine setup (src-tauri/src/setup.rs, patchpilot_parity::SetupProgress).

export type StepStatus = "todo" | "partly" | "done" | "skipped";

export interface SetupStep {
  /** `step:<n>` or `check:<name>`, stable for the whole setup. */
  key: string;
  number: number | null;
  group: string;
  title: string;
  /** Vendor app or licensing tool adapter ID, for an Open button. */
  app: string | null;
  status: StepStatus;
  /** Ticked by Patch Pilot after a rescan; false for final checks the user ticks. */
  automatic: boolean;
  open: string[];
  finished: string[];
  open_products: string[];
  after: number[];
}

export interface SetupProgress {
  steps: SetupStep[];
  done: number;
  total: number;
  score_before: number;
  score_now: number;
  templates: TemplateReadiness[];
  templates_ready: number;
  complete: boolean;
}

export interface SetupView {
  offer: boolean;
  profiles: { id: string; name: string }[];
  profile_id: string | null;
  active: {
    machine: string;
    profile: string | null;
    golden_version: number;
    started_at: number;
    finished_at: number | null;
    progress: SetupProgress;
  } | null;
}

export const setupView = () => invoke<SetupView>("setup_view");
export const setupStart = (profileId: string | null) => invoke<SetupView>("setup_start", { profileId });
export const setupTick = (key: string, done: boolean) => invoke<SetupView>("setup_tick", { key, done });
export const setupFinish = () => invoke<SetupView>("setup_finish");
export const setupClose = () => invoke<SetupView>("setup_close");
export const setupReportText = () => invoke<string>("setup_report_text");
/** Saves the report to Downloads and returns its path. */
export const setupSaveReport = () => invoke<string>("setup_save_report");

// Reaper extras (decision #52): ReaPack restore and folder settings.
export interface ReaPackRemote {
  name: string;
  url: string;
  enabled: boolean;
}
export interface ReaPackPackage {
  remote: string;
  category: string;
  package: string;
  desc: string;
  version: string;
  kind: number;
}
export type FolderSetting = { type: "key"; key: string } | { type: "explorer"; label: string };
export interface FolderFix {
  setting: FolderSetting;
  name: string;
  golden: string;
  current: string | null;
  proposed: string | null;
}
export interface ReaperExtras {
  reaper_found: boolean;
  reapack_installed: boolean;
  reapack: { missing_remotes: ReaPackRemote[]; missing_packages: ReaPackPackage[] };
  folders: FolderFix[];
}
export const reaperExtras = () => invoke<ReaperExtras | null>("reaper_extras");
export const restoreReapack = () => invoke<ReaperExtras | null>("restore_reapack");
export const fixReaperFolders = () => invoke<ReaperExtras | null>("fix_reaper_folders");

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

// History (decision #45): src-tauri/src/history.rs and patchpilot_parity::ProductChange.

export type Change = "added" | "removed" | "updated" | "downgraded";

export interface ProductChange {
  product_id: string;
  name: string;
  vendor: string;
  change: Change;
  from: string | null;
  to: string | null;
}

export interface FileLine {
  name: string;
  /** The last two folders, e.g. "REAPER/TrackTemplates". */
  folder: string;
  path: string;
}

export interface FilePreview extends FileLine {
  /** Put back; false means the change created it, so it's removed. */
  restore: boolean;
  changed_since: boolean;
  missing_backup: boolean;
}

export type HistoryEntry =
  | {
      type: "scan";
      snapshot_id: number;
      at: number;
      first: boolean;
      latest: boolean;
      plugins: number;
      changes: ProductChange[];
    }
  | { type: "files"; id: number; at: number; label: string; undo: boolean; undone: boolean; files: FileLine[] };

export interface HistoryView {
  entries: HistoryEntry[];
}

export interface GoBackPlan {
  at: number;
  files: FilePreview[];
  /** From now to then: added = reinstall, removed = uninstall. */
  plugins: ProductChange[];
}

export const history = () => invoke<HistoryView>("history");
export const undoPreview = (id: number) => invoke<FilePreview[]>("undo_preview", { id });
export const undoChange = (id: number) => invoke<HistoryView>("undo_change", { id });
export const goBackPlan = (snapshotId: number) => invoke<GoBackPlan>("go_back_plan", { snapshotId });
export const goBack = (snapshotId: number) => invoke<HistoryView>("go_back", { snapshotId });
