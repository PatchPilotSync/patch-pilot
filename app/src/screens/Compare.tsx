import { useCallback, useEffect, useMemo, useState } from "react";
import {
  addToGolden,
  bringFiles,
  fileKey,
  filesToBring,
  updateGoldenFrom,
  compareMachine,
  ignoreItems,
  ignoreSuggestions,
  unignoreSuggestions,
  isReapack,
  FORMAT_LABEL,
  groupSummary,
  openVendorApp,
  planProgress,
  planTick,
  STATE_HINT,
  STATE_LABEL,
  SUGGESTION_GROUP,
  vaultMatches,
  vaultRun,
  vendorApps,
  type BringView,
  type ComparisonRow,
  type FileToBring,
  type MachineCard,
  type MachineComparison,
  type MachinesView,
  type Matcher,
  type ParityState,
  type PluginFormat,
  type SetupProgress,
  type SetupStep,
  type SuggestionGroup,
  type VaultMatch,
  type VendorApp,
} from "../api";
import { IgnoreButton, IgnoredToggle, pathRules, productRules, useIgnores } from "../Ignore";
import { bringSummary, isReaperScript, isTemplate, Needs } from "../bring";
import { OtherFormatsNote, StepExtras, StepList, useQuickSteps, type Actions } from "./Setup";
import { formatSize } from "./VaultTriage";
import {
  Badge,
  Button,
  buildTree,
  Folder,
  FolderControls,
  FolderTree,
  inputClass,
  isConflictCopy,
  Loading,
  Notice,
  Pill,
  ScoreBar,
  scoreColor,
  timeAgo,
  treeItems,
  useFolders,
} from "../ui";

const STATE_ORDER: ParityState[] = [
  "missing",
  "incomplete",
  "outdated",
  "modified",
  "version_unknown",
  "newer_than_golden",
  "extra",
  "not_applicable",
  "in_parity",
];

/** Status dot colour for each parity state. */
const STATE_DOT: Record<ParityState, string> = {
  missing: "bg-bad",
  incomplete: "bg-warn",
  outdated: "bg-warn",
  modified: "bg-warn",
  version_unknown: "bg-warn",
  newer_than_golden: "bg-ok",
  extra: "bg-dim",
  not_applicable: "bg-dim",
  in_parity: "bg-ok",
};

/** States whose folder starts open in the diff: the ones that need work. */
const OPEN_STATES: ParityState[] = ["missing", "incomplete", "outdated", "modified", "version_unknown"];

/** Rows in a state folder above this many are split into vendor folders. */
const FLAT_MAX = 30;

/**
 * A long state group split into one folder per vendor, with ReaPack packages
 * (thousands, sometimes) in one folder of their own. Null when a flat list reads fine.
 */
function subGroups(rows: ComparisonRow[]): [string, ComparisonRow[]][] | null {
  const reapack = rows.filter((r) => isReapack(r.product_id));
  const files = rows.filter(isReaperFile);
  if (rows.length <= FLAT_MAX && reapack.length <= 1 && files.length <= 1) return null;
  const groups = byVendor(rows.filter((r) => !isReapack(r.product_id) && !isReaperFile(r)));
  if (files.length) groups.push([REAPER_FILES, files]);
  if (reapack.length) groups.push(["ReaPack", reapack]);
  return groups.length > 1 || reapack.length > 1 || files.length > 1 ? groups : null;
}

/** The folder Reaper's own files (scripts, themes, templates, key maps) go in, nested by their paths. */
const REAPER_FILES = "Reaper files";

/** A file from Reaper's resource folder; its name is its path there, e.g. `ColorThemes/Default_7.0.ReaperThemeZip`. */
const isReaperFile = (r: ComparisonRow) => /^reaper-(script|theme|config|template)\//.test(r.product_id);

/** Reaper's resource folders under friendlier names. */
const REAPER_FOLDER: Record<string, string> = {
  Scripts: "Scripts",
  ColorThemes: "Themes",
  FXChains: "FX chains",
  TrackTemplates: "Track templates",
  ProjectTemplates: "Project templates",
};

/** `Scripts/ReaTeam Scripts/Items/x.lua` -> Scripts, ReaTeam Scripts, Items. Loose files (`reaper-kb.ini`) are configuration. */
function reaperDirs(r: ComparisonRow): string[] {
  const dirs = r.name.split("/").slice(0, -1);
  if (dirs.length === 0) return ["Configuration"];
  return [REAPER_FOLDER[dirs[0]] ?? dirs[0], ...dirs.slice(1)];
}

/** Rows in one folder per vendor, A to Z. */
function byVendor(rows: ComparisonRow[]): [string, ComparisonRow[]][] {
  const by = new Map<string, ComparisonRow[]>();
  for (const r of rows) {
    const k = r.vendor || "Unknown vendor";
    by.set(k, [...(by.get(k) ?? []), r]);
  }
  return [...by.entries()].sort(([a], [b]) => a.localeCompare(b));
}

function DiffRow({
  r,
  label,
  selected,
  onSelect,
  showVendor,
  ignore,
}: {
  r: ComparisonRow;
  /** Shown instead of the name, e.g. just the file name inside a path folder. */
  label?: string;
  selected: ComparisonRow | null;
  onSelect: (r: ComparisonRow) => void;
  showVendor: boolean;
  /** Ignore it on this machine (rows that need work). */
  ignore?: IgnoreRows;
}) {
  return (
    <li
      className={`group flex items-center rounded-[5px] ${
        selected?.product_id === r.product_id ? "bg-sel" : "hover:bg-sel/50"
      }`}
    >
      <button
        onClick={() => onSelect(r)}
        className="flex h-7 min-w-0 flex-1 items-center gap-2.5 px-2 text-left"
      >
        <span className="min-w-0 flex-1 truncate" title={label ? r.name : undefined}>
          {label ?? r.name}
          {showVendor && r.vendor && <span className="ml-2 text-xs text-dim">{r.vendor}</span>}
        </span>
        {isConflictCopy(r.name) && (
          <span title="A copy a sync app (Dropbox, OneDrive...) made when two machines changed this file at once. You can delete it.">
            <Badge tone="amber">conflict copy</Badge>
          </span>
        )}
        {r.works_with_missing.length > 0 && r.state !== "missing" && (
          <span title={`Needs files this machine doesn't have: ${r.works_with_missing.join(", ")}`}>
            <Badge tone="amber">
              needs {r.works_with_missing.length} file{r.works_with_missing.length === 1 ? "" : "s"}
            </Badge>
          </span>
        )}
        {r.needed_by_templates && <Badge tone="amber">template</Badge>}
        {r.confidence === "low" && <Badge tone="amber">name only</Badge>}
        {r.missing_formats.map((f) => (
          <Badge key={f}>no {FORMAT_LABEL[f]}</Badge>
        ))}
        {(!label || versionPair(r)) && (
          <span className="w-28 shrink-0 text-right font-mono text-[11px] text-dim">{versionPair(r)}</span>
        )}
      </button>
      {ignore && OPEN_STATES.includes(r.state) && (
        <IgnoreButton disabled={ignore.busy} onIgnore={() => ignore.run(label ?? r.name, [r])} />
      )}
    </li>
  );
}

/** Ignore (decision #65) rows of a machine's comparison. */
interface IgnoreRows {
  busy: boolean;
  run: (label: string, rows: ComparisonRow[]) => void;
}

/** What to ignore for these rows: Reaper files by path (a folder as one), ReaPack as a whole, the rest by product. */
function rowRules(rows: ComparisonRow[]): Matcher[] {
  const files = rows.filter(isReaperFile);
  const reapack = rows.some((r) => isReapack(r.product_id));
  const rest = rows.filter((r) => !isReaperFile(r) && !isReapack(r.product_id));
  return [
    ...pathRules(files.map((r) => ({ root: "REAPER_RESOURCES", rel: r.name }))),
    ...(reapack ? [{ vendor: "ReaPack" }] : []),
    ...productRules(rest.map((r) => r.product_id)),
  ];
}

/** A folder header's Ignore, for its rows that need work. */
function IgnoreFolder({ label, rows, ignore }: { label: string; rows: ComparisonRow[]; ignore: IgnoreRows }) {
  const work = rows.filter((r) => OPEN_STATES.includes(r.state));
  if (!work.length) return null;
  return <IgnoreButton disabled={ignore.busy} onIgnore={() => ignore.run(label, work)} />;
}

/** A request, from the tray popover, to show one machine (and maybe its plan). */
export interface Focus {
  id: string;
  plan: boolean;
  /** Makes repeated requests for the same machine distinct. */
  at: number;
}

interface Props {
  view: MachinesView;
  focus: Focus | null;
  /** The machine the user picked. Kept by App so scans, syncs and tab switches don't lose it. */
  chosen: string | null;
  onChoose: (id: string) => void;
  onError: (e: string) => void;
}

/**
 * The last comparison of each machine, shown straight away when it's picked
 * again (or the window reopens) while a fresh one runs. Lives as long as the
 * app, which stays in the tray.
 */
const lastComparison = new Map<string, MachineComparison>();
/** Steps before Get Up To Speed has loaded, one array so effects don't rerun. */
const NO_STEPS: SetupStep[] = [];

export function Compare({ view, focus, chosen, onChoose, onError }: Props) {
  // A refresh can briefly list fewer machines (a sync landing); fall back for
  // the moment but keep the choice, so it comes back when the machine does.
  const selected = view.machines.some((m) => m.id === chosen) ? chosen : (view.machines[0]?.id ?? null);
  const [loaded, setData] = useState<MachineComparison | null>(() =>
    selected ? (lastComparison.get(selected) ?? null) : null,
  );
  // True while a fresh comparison runs behind the one shown.
  const [refreshing, setRefreshing] = useState(false);
  // Never show one machine's comparison under another machine's name.
  const data = loaded?.machine.id === selected ? loaded : null;
  const [plan, setPlan] = useState(false);
  const [rowId, setRowId] = useState<string | null>(null);
  const row = data?.comparison.rows.find((r) => r.product_id === rowId) ?? null;
  const setRow = (r: ComparisonRow | null) => setRowId(r?.product_id ?? null);
  const [wantPlan, setWantPlan] = useState(focus?.plan ?? false);
  const [busy, setBusy] = useState(false);
  const bring = useBringHere(data?.machine ?? null, view, onError);

  // Both refresh every window through `data-changed`, which reloads `view`.
  const suggest: SuggestActions = {
    busy,
    add: (ids) => {
      if (!data) return;
      setBusy(true);
      addToGolden(data.machine.id, ids).catch((e) => onError(String(e))).finally(() => setBusy(false));
    },
    update: () => {
      if (!data) return;
      setBusy(true);
      updateGoldenFrom(data.machine.id).catch((e) => onError(String(e))).finally(() => setBusy(false));
    },
    ignore: (rs) => {
      setBusy(true);
      ignoreSuggestions(rs.map((r) => ({ product: r.product_id, version: r.machine_version })))
        .catch((e) => onError(String(e)))
        .finally(() => setBusy(false));
    },
    unignore: (rs) => {
      setBusy(true);
      unignoreSuggestions(rs.map((r) => ({ product: r.product_id, version: r.machine_version })))
        .catch((e) => onError(String(e)))
        .finally(() => setBusy(false));
    },
  };

  useEffect(() => {
    if (focus) setWantPlan(focus.plan);
  }, [focus]);

  useEffect(() => {
    if (!wantPlan || !data || data.machine.id !== selected) return;
    setWantPlan(false);
    setPlan(true);
  }, [wantPlan, data, selected]);

  useEffect(() => {
    setData(selected ? (lastComparison.get(selected) ?? null) : null);
    setPlan(false);
    setRowId(null);
  }, [selected]);

  // A rescan or sync reloads `view`; the open row, checklist and folders stay as they are.
  // Comparing a big rig takes a while, so an older request (another machine, or
  // before a refresh) can finish last; only the latest one may land.
  useEffect(() => {
    if (!selected || view.golden_version === null) return;
    let current = true;
    setRefreshing(true);
    compareMachine(selected)
      .then(
        (d) => {
          lastComparison.set(d.machine.id, d);
          if (current) setData(d);
        },
        (e) => current && onError(String(e)),
      )
      .finally(() => current && setRefreshing(false));
    return () => {
      current = false;
    };
  }, [selected, view, onError]);

  return (
    <div className="flex h-full min-h-0">
      <aside className="flex w-52 shrink-0 flex-col border-r border-line bg-panel">
        <div className="px-4 pb-1.5 pt-3 text-[11px] text-dim">Machines</div>
        <ul className="flex flex-1 flex-col gap-0.5 overflow-auto px-2">
          {view.machines.map((m) => (
            <MachineItem key={m.id} m={m} active={m.id === selected} onClick={() => onChoose(m.id)} />
          ))}
        </ul>
        {view.machines.length < 2 && (
          <p className="p-4 text-[11px] leading-relaxed text-dim">
            Your other machines show up here once they're linked. Use Link a machine in the top bar.
          </p>
        )}
      </aside>

      <section className="min-w-0 flex-1 overflow-auto">
        {view.golden_version === null ? (
          <Empty text="Make a Primary Rig first. Scan your main machine and choose Make Primary." />
        ) : !data ? (
          selected ? (
            <Loading text="Comparing with the Primary Rig…" compact />
          ) : (
            <Empty text="No machines yet." />
          )
        ) : plan ? (
          <PlanView
            id={data.machine.id}
            name={data.machine.name}
            here={data.machine.is_this_machine}
            refresh={view}
            onError={onError}
            onBack={() => setPlan(false)}
          />
        ) : (
          <Diff
            data={data}
            selected={row}
            onSelect={setRow}
            suggest={suggest}
            bring={bring}
            onPlan={() => setPlan(true)}
            onError={onError}
            refreshing={refreshing}
          />
        )}
      </section>

      {/* Up To Speed is a checklist with nothing to select, so it gets the room. */}
      {!plan && (
        <aside className="w-64 shrink-0 overflow-auto border-l border-line bg-panel">
          {row && data ? (
            <Detail
              row={row}
              machine={data.machine}
              version={data.golden_version}
              suggested={data.suggested.includes(row.product_id)}
              suggest={suggest}
              bring={bring}
            />
          ) : (
            <p className="p-4 text-xs text-dim">Select a row to see its versions here and on the Primary Rig.</p>
          )}
        </aside>
      )}
    </div>
  );
}

function MachineItem({ m, active, onClick }: { m: MachineCard; active: boolean; onClick: () => void }) {
  return (
    <li>
      <button
        onClick={onClick}
        className={`flex w-full flex-col gap-1.5 rounded-md px-2.5 py-2 text-left ${
          active ? "bg-sel" : "hover:bg-sel/50"
        }`}
      >
        <span className="flex w-full items-baseline gap-2">
          <span className="min-w-0 flex-1 truncate font-medium">{m.name}</span>
          <span className={`font-semibold tabular-nums ${scoreColor(m.score)}`}>{m.score ?? "–"}</span>
        </span>
        <ScoreBar score={m.score} className="w-full" />
        <span className="block w-full truncate text-[11px] text-dim">
          {m.os === "macos" ? "macOS" : "Windows"}
          {m.is_this_machine ? " · this machine" : ""}
          {m.golden_source ? " · Primary machine" : ""}
          {m.profile ? ` · ${m.profile.name}` : ""}
        </span>
      </button>
    </li>
  );
}

function Empty({ text }: { text: string }) {
  return <p className="p-6 text-dim">{text}</p>;
}

function Diff({
  data,
  selected,
  onSelect,
  suggest,
  bring,
  onPlan,
  onError,
  refreshing,
}: {
  data: MachineComparison;
  selected: ComparisonRow | null;
  onSelect: (r: ComparisonRow) => void;
  suggest: SuggestActions;
  bring: BringHere;
  onPlan: () => void;
  onError: (e: string) => void;
  /** The last comparison is shown while a fresh one runs. */
  refreshing: boolean;
}) {
  const { machine, comparison: c } = data;
  const [ignoring, setIgnoring] = useState(false);
  // `data-changed` reloads the comparison without what was ignored.
  const ignore: IgnoreRows = {
    busy: ignoring,
    run: (label, rs) => {
      setIgnoring(true);
      ignoreItems(machine.id, label, rowRules(rs))
        .catch((e) => onError(String(e)))
        .finally(() => setIgnoring(false));
    },
  };
  const [states, setStates] = useState<Set<ParityState>>(new Set());
  const [vendor, setVendor] = useState("");
  const [format, setFormat] = useState<PluginFormat | "">("");
  const [showParity, setShowParity] = useState(false);
  // Problems start open; the rest (In parity, Not in Primary, ...) start folded.
  const folders = useFolders("compare", (id) => !id.includes("/") && OPEN_STATES.includes(id as ParityState));

  const counts = useMemo(() => {
    const n = new Map<ParityState, number>();
    for (const r of c.rows) n.set(r.state, (n.get(r.state) ?? 0) + 1);
    return n;
  }, [c]);
  const vendors = useMemo(() => [...new Set(c.rows.map((r) => r.vendor).filter(Boolean))].sort(), [c]);
  const formats = useMemo(
    () => [...new Set(c.rows.flatMap((r) => [...r.golden_items, ...r.machine_items].map((i) => i.format)))]
      .filter((f): f is PluginFormat => f !== null),
    [c],
  );

  const rows = c.rows.filter(
    (r) =>
      (states.size ? states.has(r.state) : showParity || r.state !== "in_parity") &&
      (!vendor || r.vendor === vendor) &&
      (!format || [...r.golden_items, ...r.machine_items].some((i) => i.format === format)),
  );
  const shownStates = STATE_ORDER.filter((s) => rows.some((r) => r.state === s));
  const filtered = states.size > 0 || vendor !== "" || format !== "";
  const toggle = (s: ParityState) =>
    setStates((prev) => {
      const next = new Set(prev);
      if (next.has(s)) next.delete(s);
      else next.add(s);
      return next;
    });
  const hasWork = c.rows.some((r) => ["missing", "incomplete", "outdated"].includes(r.state));

  return (
    <div className="flex flex-col gap-3 px-5 py-4">
      <header className="flex items-center gap-4">
        <span className={`text-3xl font-semibold leading-none tracking-tight tabular-nums ${scoreColor(c.score)}`}>
          {c.score}
        </span>
        <div className="min-w-0 flex-1">
          <h1 className="truncate text-sm font-semibold">{machine.name}</h1>
          <p className="truncate text-xs text-dim">
            vs Primary Rig v{data.golden_version}
            {machine.golden_source ? " (saved from this machine)" : ""} ·{" "}
            {machine.profile ? `${machine.profile.name} profile` : "whole rig"} · scanned {timeAgo(machine.scanned_at)}
            {refreshing && " · updating…"}
          </p>
        </div>
        {hasWork &&
          (machine.is_this_machine ? (
            <Button onClick={onPlan}>Get Up To Speed</Button>
          ) : (
            // Installing happens on that machine, so nothing here asks for a click.
            <div className="flex min-w-0 max-w-80 shrink flex-col items-end gap-0.5 text-right text-[11px] text-dim">
              <span className="max-w-full truncate" title={`Open Patch Pilot on ${machine.name} to get it up to speed.`}>
                Get Up To Speed runs on that machine
              </span>
              <Button variant="link" onClick={onPlan} className="h-5 px-1.5 text-[11px]">
                See its checklist
              </Button>
            </div>
          ))}
      </header>

      {c.reasons.length > 0 ? (
        <div className="flex flex-col gap-1.5">
          <div className="flex h-1.5 overflow-hidden rounded-full bg-track" aria-hidden="true">
            <span className="bg-ok" style={{ width: `${c.score}%` }} />
            {c.reasons.map((r, i) => (
              <span
                key={r.text}
                className="bg-bad"
                style={{ width: `${r.points}%`, opacity: Math.max(1 - i * 0.15, 0.35) }}
              />
            ))}
          </div>
          <ul className="flex flex-wrap gap-x-3 gap-y-0.5 text-xs text-dim">
            {c.reasons.map((r) => (
              <li key={r.text}>
                <span className="tabular-nums text-fg">−{Math.round(r.points)}</span> {r.text}
              </li>
            ))}
          </ul>
        </div>
      ) : (
        <p className="text-xs text-ok">At parity with the Primary Rig.</p>
      )}

      <div className="-mx-2 flex flex-col">
        {bring.scripts.files.length > 0 && (
          <Notice
            text={
              <>
                {bring.scripts.files.length.toLocaleString()} Reaper script file
                {bring.scripts.files.length === 1 ? "" : "s"}{" "}
                {bring.from ? `on ${machine.name} differ from this machine's` : "from the Primary Rig aren't here yet"}
                {bring.scripts.tooLarge > 0 && (
                  <span className="text-dim"> ({bring.scripts.tooLarge} too large to send)</span>
                )}
              </>
            }
            actions={
              <Button
                variant="link"
                disabled={bring.busy}
                onClick={() => bring.bring(bring.scripts.files, bring.scripts.tooLarge)}
                className="h-6 shrink-0 px-2"
                title={`Every script and ReaPack package, each with the files it needs. ${bring.hint}`}
              >
                {bring.from ? "Copy all Reaper scripts here" : "Bring over all Reaper scripts"}
              </Button>
            }
          />
        )}

        {bring.templates.files.length > 0 && (
          <Notice
            text={
              <>
                {bring.templates.files.length.toLocaleString()} template
                {bring.templates.files.length === 1 ? "" : "s"}{" "}
                {bring.from ? `on ${machine.name} differ from this machine's` : "from the Primary Rig aren't here yet"}
                {bring.templates.tooLarge > 0 && (
                  <span className="text-dim"> ({bring.templates.tooLarge} too large to send)</span>
                )}
              </>
            }
            actions={
              <Button
                variant="link"
                disabled={bring.busy}
                onClick={() => bring.bring(bring.templates.files, bring.templates.tooLarge)}
                className="h-6 shrink-0 px-2"
                title={`Every Reaper track and project template and Ableton template. ${bring.hint}`}
              >
                {bring.from ? "Copy all templates here" : "Bring over all templates"}
              </Button>
            }
          />
        )}

        {bring.message && (
          <p className="px-2 py-1 text-xs text-ok">
            {bring.message.text}
            {bring.message.warn && <span className="text-warn"> {bring.message.warn}</span>}
          </p>
        )}

        <TemplatesNotice data={data} selected={selected} onSelect={onSelect} onPlan={onPlan} />
        <SuggestionsNotice data={data} selected={selected} onSelect={onSelect} suggest={suggest} />
        <div className="px-2">
          <IgnoredToggle ignores={data.ignores} machine={machine.is_this_machine ? null : machine.id} onError={onError} />
        </div>
      </div>

      {/* Filters by state, then the rest of the filters with the folders' controls, so neither row wraps. */}
      <div className="-mx-1 flex flex-wrap items-center gap-0.5">
        {STATE_ORDER.filter((s) => s !== "in_parity" && counts.get(s)).map((s) => (
          <Pill key={s} active={states.has(s)} onClick={() => toggle(s)}>
            {stateLabel(s, machine, data.golden_version)} <span className="tabular-nums opacity-70">{counts.get(s)}</span>
          </Pill>
        ))}
      </div>
      <div className="-mt-1.5 flex items-center gap-1.5 border-b border-line pb-2">
        {!states.size && counts.get("in_parity") ? (
          <label className="mr-1.5 flex shrink-0 items-center gap-1.5 text-xs text-dim">
            <input
              type="checkbox"
              className="accent-accent"
              checked={showParity}
              onChange={(e) => setShowParity(e.target.checked)}
            />
            {STATE_LABEL.in_parity} <span className="tabular-nums opacity-70">{counts.get("in_parity") ?? 0}</span>
          </label>
        ) : null}
        <select
          value={vendor}
          onChange={(e) => setVendor(e.target.value)}
          aria-label="Vendor"
          className={`h-6 max-w-32 ${inputClass}`}
        >
          <option value="">All vendors</option>
          {vendors.map((v) => (
            <option key={v}>{v}</option>
          ))}
        </select>
        <select
          value={format}
          onChange={(e) => setFormat(e.target.value as PluginFormat | "")}
          aria-label="Format"
          className={`h-6 ${inputClass}`}
        >
          <option value="">All formats</option>
          {formats.map((f) => (
            <option key={f} value={f}>
              {FORMAT_LABEL[f]}
            </option>
          ))}
        </select>
        <span className="flex-1" />
        <FolderControls ids={shownStates.map((st) => st as string)} folders={folders} className="-mr-2 shrink-0" />
      </div>

      <div className="-mx-2 -mt-1 flex flex-col">
        {shownStates.map((s) => {
          const all = rows.filter((r) => r.state === s);
          const subs = subGroups(all);
          return (
            <Folder
              key={s}
              title={
                <span className="flex items-center gap-2">
                  <span className={`h-1.5 w-1.5 shrink-0 rounded-full ${STATE_DOT[s]}`} aria-hidden="true" />
                  {stateLabel(s, machine, data.golden_version)}
                </span>
              }
              count={all.length}
              className="rounded-[5px] px-2"
              open={filtered || folders.isOpen(s)}
              onToggle={() => folders.toggle(s)}
            >
              {subs ? (
                subs.map(([name, rs]) => {
                  const id = `${s}/${name}`;
                  return (
                    <Folder
                      key={id}
                      title={name}
                      count={rs.length}
                      className="rounded-[5px] pl-6 pr-2"
                      open={folders.isOpen(id)}
                      onToggle={() => folders.toggle(id)}
                      actions={
                        <>
                          <IgnoreFolder label={name === REAPER_FILES ? "Reaper files" : name} rows={rs} ignore={ignore} />
                          {name === REAPER_FILES && <BringButton rows={rs} bring={bring} />}
                        </>
                      }
                    >
                      {name === REAPER_FILES ? (
                        <FolderTree
                          node={buildTree(rs, reaperDirs, id)}
                          folders={folders}
                          indent={40}
                          actions={(node) => (
                            <>
                              <IgnoreFolder label={node.name} rows={treeItems(node)} ignore={ignore} />
                              <BringButton rows={treeItems(node)} bring={bring} />
                            </>
                          )}
                          renderItems={(items, indent) => (
                            <ul style={{ paddingLeft: indent }}>
                              {items.map((r) => (
                                <DiffRow
                                  key={r.product_id}
                                  r={r}
                                  label={r.name.split("/").pop()}
                                  selected={selected}
                                  onSelect={onSelect}
                                  showVendor={false}
                                  ignore={ignore}
                                />
                              ))}
                            </ul>
                          )}
                        />
                      ) : (
                        <ul className="pl-8">
                          {rs.map((r) => (
                            <DiffRow
                              key={r.product_id}
                              r={r}
                              selected={selected}
                              onSelect={onSelect}
                              showVendor={false}
                              ignore={ignore}
                            />
                          ))}
                        </ul>
                      )}
                    </Folder>
                  );
                })
              ) : (
                <ul className="pl-4">
                  {all.map((r) => (
                    <DiffRow key={r.product_id} r={r} selected={selected} onSelect={onSelect} showVendor ignore={ignore} />
                  ))}
                </ul>
              )}
            </Folder>
          );
        })}
        {!rows.length && (states.size > 0 || vendor || format) && (
          <p className="px-2 text-dim">Nothing to show with these filters.</p>
        )}
        {c.unresolved > 0 && (
          <p className="px-2 text-[11px] text-dim">
            {c.unresolved} file{c.unresolved === 1 ? "" : "s"} on this machine couldn't be identified, so{" "}
            {c.unresolved === 1 ? "it isn't" : "they aren't"} compared.
          </p>
        )}
      </div>
    </div>
  );
}

/** One plug-in that templates on this machine use but it doesn't have. */
interface NeededPlugin {
  id: string;
  name: string;
  vendor: string;
  /** Its comparison row, when the Primary Rig has it. */
  row: ComparisonRow | null;
  /** Template paths, e.g. `FXChains/Bus Glue.RfxChain`. */
  templates: string[];
}

/** The not-ready templates of one app, with the plug-ins they lack. */
interface AppTemplates {
  app: string;
  templates: number;
  plugins: NeededPlugin[];
}

/** The app a template product belongs to, from its ID prefix: `reaper-template/...` -> Reaper. */
function templateApp(productId: string): string {
  const app = productId.split("/")[0].split("-")[0];
  return app ? app.charAt(0).toUpperCase() + app.slice(1) : "Other";
}

/**
 * Templates that would open with empty plug-in slots: one line, expandable to
 * the plug-ins they lack, grouped by the app the templates belong to.
 */
function TemplatesNotice({
  data,
  selected,
  onSelect,
  onPlan,
}: {
  data: MachineComparison;
  selected: ComparisonRow | null;
  onSelect: (r: ComparisonRow) => void;
  onPlan: () => void;
}) {
  const c = data.comparison;
  const [open, setOpen] = useState(false);
  const folders = useFolders("templates");
  const apps = useMemo(() => {
    const rows = new Map(c.rows.map((r) => [r.product_id, r]));
    const byApp = new Map<string, { templates: number; plugins: Map<string, NeededPlugin> }>();
    for (const t of c.templates) {
      if (!t.missing.length) continue;
      const app = templateApp(t.product_id);
      let group = byApp.get(app);
      if (!group) byApp.set(app, (group = { templates: 0, plugins: new Map() }));
      group.templates += 1;
      for (const id of t.missing) {
        let p = group.plugins.get(id);
        if (!p) {
          const row = rows.get(id) ?? null;
          p = { id, ...(row ? { name: row.name, vendor: row.vendor } : nameFromId(id)), row, templates: [] };
          group.plugins.set(id, p);
        }
        p.templates.push(t.name);
      }
    }
    return [...byApp]
      .map(([app, g]): AppTemplates => ({
        app,
        templates: g.templates,
        plugins: [...g.plugins.values()].sort(
          (a, b) => b.templates.length - a.templates.length || a.name.localeCompare(b.name),
        ),
      }))
      .sort((a, b) => a.app.localeCompare(b.app));
  }, [c]);
  if (!apps.length) return null;

  const t = apps.reduce((n, a) => n + a.templates, 0);
  const p = new Set(apps.flatMap((a) => a.plugins.map((n) => n.id))).size;
  const anyInGolden = apps.some((a) => a.plugins.some((n) => n.row));
  const anyOutside = apps.some((a) => a.plugins.some((n) => !n.row));
  const who = data.machine.is_this_machine ? "this machine doesn't" : `${data.machine.name} doesn't`;
  return (
    <Notice
      tone="warn"
      text={
        <>
          {t} {apps.length === 1 ? `${apps[0].app} ` : ""}template{t === 1 ? "" : "s"} need{t === 1 ? "s" : ""}{" "}
          {p === 1 ? "a plug-in" : `${p} plug-ins`} {who} have
        </>
      }
      open={open}
      onToggle={() => setOpen(!open)}
    >
      <div className="flex items-center gap-2">
        <p className="min-w-0 flex-1 text-dim">
          They'd open with empty plug-in slots.
          {anyOutside && " Only plug-ins in the Primary Rig count in the score."}
        </p>
        {anyInGolden && (
          <Button variant="link" onClick={onPlan} className="h-6 shrink-0 px-2">
            {data.machine.is_this_machine ? "Open Up To Speed" : "See its checklist"}
          </Button>
        )}
      </div>
      <div className="-mx-2">
        {apps.map((a) => (
          <Folder
            key={a.app}
            title={a.app}
            count={a.plugins.length}
            className="px-2"
            // A lone app needs no extra click.
            open={apps.length === 1 || folders.isOpen(a.app)}
            onToggle={() => folders.toggle(a.app)}
            badges={
              <span className="text-[11px] text-dim">
                {a.templates} template{a.templates === 1 ? "" : "s"}
              </span>
            }
          >
            <AppGroup group={a} selected={selected} onSelect={onSelect} />
          </Folder>
        ))}
      </div>
    </Notice>
  );
}

function AppGroup({
  group: g,
  selected,
  onSelect,
}: {
  group: AppTemplates;
  selected: ComparisonRow | null;
  onSelect: (r: ComparisonRow) => void;
}) {
  return (
    <div>
      <ul className="flex flex-col pl-4">
        {g.plugins.map((n) => {
          const body = (
            <>
              <span className="shrink-0">
                {n.name}
                {n.vendor && <span className="ml-1.5 text-dim">{n.vendor}</span>}
              </span>
              {!n.row && <Badge>not in Primary Rig</Badge>}
              <span className="min-w-0 flex-1 truncate text-right text-[11px] text-dim">
                {n.templates.length === 1
                  ? templateLabel(n.templates[0])
                  : `${n.templates.length} templates · ${n.templates.map(templateLabel).join(", ")}`}
              </span>
            </>
          );
          const look = "flex h-6 w-full items-center gap-2 rounded-[5px] px-2 text-left";
          return (
            <li key={n.id} title={`Used by:\n${n.templates.join("\n")}`}>
              {n.row ? (
                <button
                  onClick={() => n.row && onSelect(n.row)}
                  className={`${look} ${selected?.product_id === n.id ? "bg-sel" : "hover:bg-sel/50"}`}
                >
                  {body}
                </button>
              ) : (
                <div className={look}>{body}</div>
              )}
            </li>
          );
        })}
      </ul>
    </div>
  );
}

/** `FXChains/Vocals/Bus Glue.RfxChain` -> `Bus Glue`. */
function templateLabel(path: string): string {
  const file = path.split("/").pop() ?? path;
  return file.replace(/\.(rfxchain|rtracktemplate|rpp)$/i, "");
}

/** A readable name for a product ID (`acme-audio/acme-verb`) the comparison has no row for. */
function nameFromId(id: string): { name: string; vendor: string } {
  const words = (s: string) => s.split("-").map((w) => w.charAt(0).toUpperCase() + w.slice(1)).join(" ");
  const [vendor, name] = id.includes("/") ? id.split("/", 2) : ["unknown", id];
  return { name: words(name), vendor: vendor === "unknown" ? "" : words(vendor) };
}

function versionPair(r: ComparisonRow): string {
  const g = r.golden_version ?? "";
  const m = r.machine_version ?? "";
  if (r.state === "missing") return g;
  if (r.state === "extra" || g === m) return m;
  return `${m || "?"} → ${g || "?"}`;
}

/**
 * The Primary Rig is a saved copy of the machine it was made from, so on that
 * machine "Not in Primary" reads as missing from itself: say "new since" instead.
 */
function stateLabel(s: ParityState, m: MachineCard, version: number): string {
  if (m.golden_source && s === "extra") return `New since v${version}`;
  return STATE_LABEL[s];
}

function stateHint(s: ParityState, m: MachineCard, version: number): string {
  if (m.golden_source && s === "extra")
    return `Added or found here after Primary Rig v${version} was saved. Doesn't affect the score.`;
  if (m.golden_source && s === "newer_than_golden") return `Updated here after Primary Rig v${version} was saved.`;
  return STATE_HINT[s];
}

/** "Propose to Primary Rig" (M9): add or ignore what a machine has that the Primary Rig doesn't. */
interface SuggestActions {
  busy: boolean;
  add: (productIds: string[]) => void;
  /** The machine's whole latest scan becomes the next Primary Rig version. */
  update: () => void;
  ignore: (rows: ComparisonRow[]) => void;
  /** Suggest ignored ones again. */
  unignore: (rows: ComparisonRow[]) => void;
}

/** "1 script" / "8,071 scripts and effects" for a group of ReaPack rows. */
function reapackLabel(rows: ComparisonRow[]): string {
  const n = rows.filter((r) => r.product_id.startsWith("reapack/")).length;
  return `ReaPack: ${n.toLocaleString()} package${n === 1 ? "" : "s"}`;
}

/**
 * Products on this machine that the Primary Rig lacks or has an older version
 * of, each with Add and Ignore. Collapsed by default, like the templates line.
 */
function SuggestionsNotice({
  data,
  selected,
  onSelect,
  suggest,
}: {
  data: MachineComparison;
  selected: ComparisonRow | null;
  onSelect: (r: ComparisonRow) => void;
  suggest: SuggestActions;
}) {
  const [open, setOpen] = useState(false);
  const folders = useFolders("suggestions");
  const rows = data.comparison.rows.filter((r) => data.suggested.includes(r.product_id));
  const ignored = data.comparison.rows.filter((r) => data.suggested_ignored.includes(r.product_id));
  if (!rows.length) return <IgnoredSuggestions rows={ignored} suggest={suggest} />;
  // By kind, then by unit: a script package or all of ReaPack is one row.
  const groups = new Map<SuggestionGroup, Map<string, ComparisonRow[]>>();
  for (const r of rows) {
    const u = data.suggestion_units[r.product_id] ?? { group: "other", key: r.product_id, label: r.name };
    const units = groups.get(u.group) ?? new Map<string, ComparisonRow[]>();
    units.set(u.key, [...(units.get(u.key) ?? []), r]);
    groups.set(u.group, units);
  }
  const order = data.machine.suggestion_groups.map(([g]) => g);
  const n = data.machine.suggestions;
  const firstRows = [...(groups.get(order[0])?.values() ?? [])][0] ?? [];
  const firstLabel = firstRows[0] ? (data.suggestion_units[firstRows[0].product_id]?.label ?? firstRows[0].name) : "";
  const what = n === 1 ? firstLabel : groupSummary(data.machine.suggestion_groups);
  const where = data.machine.is_this_machine ? "this machine" : data.machine.name;
  const source = data.machine.golden_source;
  const v = data.golden_version;
  const unitRow = (key: string, rs: ComparisonRow[]) => {
    const reapack = isReapack(rs[0].product_id);
    if (rs.length === 1 && !reapack) return null;
    return (
      <li key={key} className="flex h-7 items-center gap-2 rounded-[5px] px-2 hover:bg-sel/50">
        <span className="min-w-0 flex-1 truncate">
          {reapack ? reapackLabel(rs) : data.suggestion_units[rs[0].product_id]?.label}
          <span className="ml-2 text-dim">
            {reapack ? "scripts, effects and themes; ReaPack installs them" : `${rs.length.toLocaleString()} files`}
          </span>
        </span>
        <Button variant="quiet" disabled={suggest.busy} onClick={() => suggest.ignore(rs)} className="h-6 px-2">
          Ignore
        </Button>
        <Button disabled={suggest.busy} onClick={() => suggest.add(rs.map((r) => r.product_id))} className="h-6 px-2.5">
          Add
        </Button>
      </li>
    );
  };
  const suggestionRow = (r: ComparisonRow, label?: string) => (
    <li
      key={r.product_id}
      className={`flex h-7 items-center gap-2 rounded-[5px] px-2 ${
        selected?.product_id === r.product_id ? "bg-sel" : "hover:bg-sel/50"
      }`}
    >
      <button onClick={() => onSelect(r)} className="flex min-w-0 flex-1 items-center gap-2 text-left">
        <span className="min-w-0 flex-1 truncate" title={label ? r.name : undefined}>
          {label ?? r.name}
          {!label && r.vendor && <span className="ml-2 text-dim">{r.vendor}</span>}
        </span>
        {isConflictCopy(r.name) && <Badge tone="amber">conflict copy</Badge>}
        <span className="shrink-0 font-mono text-[11px] text-dim">
          {r.state !== "extra"
            ? `${r.golden_version ?? "?"} → ${r.machine_version ?? "?"}`
            : source
              ? `new since v${v}`
              : "not in Primary Rig"}
        </span>
      </button>
      <Button variant="quiet" disabled={suggest.busy} onClick={() => suggest.ignore([r])} className="h-6 px-2">
        Ignore
      </Button>
      <Button disabled={suggest.busy} onClick={() => suggest.add([r.product_id])} className="h-6 px-2.5">
        Add
      </Button>
    </li>
  );
  return (
    <Notice
      text={
        source ? `New on ${where} since Primary Rig v${v}: ${what}` : `Could go into the Primary Rig from ${where}: ${what}`
      }
      open={open}
      onToggle={() => setOpen(!open)}
      toggle={["Review", "Hide"]}
    >
      <div className="flex items-center gap-2">
        <p className="min-w-0 flex-1 text-dim">
          {source
            ? `Update makes v${v + 1} from ${data.machine.name}'s latest scan; Add takes one at a time. Ignore stops suggesting it.`
            : `Add puts ${data.machine.name}'s copy in a new Primary Rig version for every machine. Ignore stops suggesting it.`}
        </p>
        {n > 1 && (
          <Button
            variant="link"
            disabled={suggest.busy}
            onClick={() => suggest.add(rows.map((r) => r.product_id))}
            className="h-6 shrink-0 px-2"
          >
            Add all
          </Button>
        )}
        {source && (
          <Button disabled={suggest.busy} onClick={suggest.update} className="h-6 shrink-0 px-2.5">
            Update Primary Rig
          </Button>
        )}
      </div>
      <ul className="-mx-2">
        {order.map((g) => {
          const units = [...(groups.get(g) ?? new Map<string, ComparisonRow[]>()).entries()];
          const singles = units.filter(([, rs]) => rs.length === 1 && !isReapack(rs[0].product_id)).map(([, rs]) => rs[0]);
          const multi = units.filter(([, rs]) => rs.length > 1 || isReapack(rs[0].product_id));
          const sub = singles.length > FLAT_MAX ? subGroups(singles) : null;
          const count = data.machine.suggestion_groups.find(([x]) => x === g)?.[1] ?? units.length;
          return (
            <li key={g}>
              <Folder
                title={SUGGESTION_GROUP[g][2]}
                count={count}
                className="rounded-[5px] px-2"
                open={folders.isOpen(g)}
                onToggle={() => folders.toggle(g)}
                actions={
                  <IgnoreButton
                    disabled={suggest.busy}
                    title="Stop suggesting these versions for the Primary Rig, on every linked machine."
                    onIgnore={() => suggest.ignore(units.flatMap(([, rs]) => rs))}
                  />
                }
              >
                <ul className="pl-4">
                  {multi.map(([key, rs]) => unitRow(key, rs))}
                  {sub
                    ? sub.map(([vendor, rs]) => (
                        <li key={vendor}>
                          <Folder
                            title={vendor}
                            count={rs.length}
                            className="rounded-[5px] px-2"
                            open={folders.isOpen(`${g}/${vendor}`)}
                            onToggle={() => folders.toggle(`${g}/${vendor}`)}
                          >
                            {vendor === REAPER_FILES ? (
                              <FolderTree
                                node={buildTree(rs, reaperDirs, `${g}/files`)}
                                folders={folders}
                                indent={22}
                                renderItems={(items, indent) => (
                                  <ul style={{ paddingLeft: indent - 8 }}>
                                    {items.map((r) => suggestionRow(r, r.name.split("/").pop()))}
                                  </ul>
                                )}
                              />
                            ) : (
                              <ul className="pl-4">{rs.map((r) => suggestionRow(r))}</ul>
                            )}
                          </Folder>
                        </li>
                      ))
                    : singles.map((r) => suggestionRow(r))}
                </ul>
              </Folder>
            </li>
          );
        })}
      </ul>
      <IgnoredSuggestions rows={ignored} suggest={suggest} />
    </Notice>
  );
}

/** Ignored suggestions shown on this machine, to suggest again. */
const IGNORED_SHOWN = 20;

function IgnoredSuggestions({ rows, suggest }: { rows: ComparisonRow[]; suggest: SuggestActions }) {
  const [open, setOpen] = useState(false);
  if (!rows.length) return null;
  return (
    <div className="text-[11px]">
      <button
        type="button"
        onClick={() => setOpen((o) => !o)}
        aria-expanded={open}
        className="text-dim underline-offset-2 hover:text-fg hover:underline"
      >
        {rows.length.toLocaleString()} ignored suggestion{rows.length === 1 ? "" : "s"}
        {open ? "" : "…"}
      </button>
      {open && (
        <ul className="mt-1 flex flex-col">
          {rows.slice(0, IGNORED_SHOWN).map((r) => (
            <li key={r.product_id} className="flex h-7 items-center gap-2 rounded-[5px] px-1 hover:bg-sel/50">
              <span className="min-w-0 flex-1 truncate text-xs" title={r.name}>
                {r.name}
                {r.machine_version && <span className="ml-2 font-mono text-dim">{r.machine_version}</span>}
              </span>
              <Button variant="quiet" className="h-6 px-2" disabled={suggest.busy} onClick={() => suggest.unignore([r])}>
                Suggest again
              </Button>
            </li>
          ))}
          <li className="flex h-7 items-center gap-2 px-1">
            <span className="min-w-0 flex-1 text-dim">
              {rows.length > IGNORED_SHOWN ? `and ${(rows.length - IGNORED_SHOWN).toLocaleString()} more` : ""}
            </span>
            {rows.length > 1 && (
              <Button variant="quiet" className="h-6 px-2" disabled={suggest.busy} onClick={() => suggest.unignore(rows)}>
                Suggest all again
              </Button>
            )}
          </li>
        </ul>
      )}
    </div>
  );
}

function Detail({
  row,
  machine,
  version,
  suggested,
  suggest,
  bring,
}: {
  row: ComparisonRow;
  machine: MachineCard;
  version: number;
  suggested: boolean;
  suggest: SuggestActions;
  bring: BringHere;
}) {
  const file = bring.file(row.product_id);
  const along = file ? bring.needs.withNeeds([fileKey(file)]).size - 1 : 0;
  return (
    <div className="flex flex-col gap-4 p-4">
      <div>
        <div className="text-sm font-semibold">{row.name}</div>
        <div className="text-xs text-dim">{row.vendor || "Unknown vendor"}</div>
      </div>
      <p className="text-xs leading-relaxed">
        <span className="mr-1.5 inline-flex items-center gap-1.5 font-medium">
          <span className={`h-1.5 w-1.5 rounded-full ${STATE_DOT[row.state]}`} aria-hidden="true" />
          {stateLabel(row.state, machine, version)}.
        </span>
        <span className="text-dim">{stateHint(row.state, machine, version)}</span>
      </p>
      <Side title="Primary Rig" items={row.golden_items} />
      <Side title={machine.name} items={row.machine_items} />
      {row.product_id.startsWith("presets/") && (
        <p className="text-xs leading-relaxed text-dim">
          A preset folder with files in it (empty ones are skipped). Patch Pilot compares a fingerprint of its files to
          spot changes; it never copies or changes them.
        </p>
      )}
      {row.works_with.length > 0 && (
        <div className="text-xs leading-relaxed">
          <p className="text-dim">
            Works together with {row.works_with.length} other file{row.works_with.length === 1 ? "" : "s"} (its
            ReaPack package, files it loads, or a theme's images).{" "}
            {row.works_with_missing.length > 0 ? (
              <span className="text-warn">
                {row.works_with_missing.length} {row.works_with_missing.length === 1 ? "isn't" : "aren't"} on{" "}
                {machine.name}, so it may not work there.
              </span>
            ) : (
              `All of them are on ${machine.name}.`
            )}
          </p>
          <ul className="mt-1 flex flex-col">
            {row.works_with.slice(0, 8).map((f) => (
              <li
                key={f}
                className={`truncate font-mono text-[11px] ${row.works_with_missing.includes(f) ? "text-warn" : "text-dim"}`}
                title={f}
              >
                {f.slice(f.lastIndexOf("/") + 1)}
              </li>
            ))}
            {row.works_with.length > 8 && (
              <li className="text-[11px] text-dim">and {row.works_with.length - 8} more</li>
            )}
          </ul>
        </div>
      )}
      {file && (
        <div className="flex flex-col gap-1.5">
          <Button disabled={bring.busy} onClick={() => bring.bring([file])} className="self-start">
            {bring.from ? "Copy to this machine" : "Bring over to this machine"}
          </Button>
          <p className="text-[11px] leading-relaxed text-dim">
            {along > 0
              ? `Comes with the ${along} file${along === 1 ? "" : "s"} it needs. `
              : ""}
            {bring.hint}
          </p>
        </div>
      )}
      {row.needed_by_templates && (
        <p className="text-xs text-dim">A template uses this plug-in, so it counts three times in the score.</p>
      )}
      {row.confidence === "low" && (
        <p className="text-xs text-dim">Identified from the file name only, so the match may be wrong.</p>
      )}
      {suggested && (
        <div className="flex flex-col gap-1.5">
          <div className="flex flex-wrap gap-1.5">
            <Button disabled={suggest.busy} onClick={() => suggest.add([row.product_id])}>
              Add {machine.name}'s copy to Primary Rig
            </Button>
            <Button variant="quiet" disabled={suggest.busy} onClick={() => suggest.ignore([row])}>
              Ignore
            </Button>
          </div>
          <p className="text-[11px] leading-relaxed text-dim">
            Makes a new Primary Rig version with it. Machines without it are then asked to install it.
          </p>
        </div>
      )}
    </div>
  );
}

function Side({ title, items }: { title: string; items: ComparisonRow["golden_items"] }) {
  return (
    <div>
      <div className="mb-1 text-[11px] text-dim">{title}</div>
      {items.length === 0 ? (
        <p className="text-xs text-bad">Not installed</p>
      ) : (
        <ul className="flex flex-col gap-1">
          {items.map((i, n) =>
            i.location ? (
              <li key={n} className="flex flex-col text-xs">
                <span className="flex items-center justify-between">
                  <span>
                    Preset folder <span className="text-dim">{i.platform === "macos" ? "macOS" : "Windows"}</span>
                  </span>
                  {i.size !== null && <span className="text-[11px] text-dim">{formatSize(i.size)}</span>}
                </span>
                <span className="break-all font-mono text-[11px] text-dim">{i.location}</span>
              </li>
            ) : (
              <li key={n} className="flex items-center justify-between text-xs">
                <span>
                  {i.format ? FORMAT_LABEL[i.format] : "—"}{" "}
                  <span className="text-dim">{i.platform === "macos" ? "macOS" : "Windows"}</span>
                </span>
                <span className="font-mono text-[11px] text-dim">{i.version ?? "?"}</span>
              </li>
            ),
          )}
        </ul>
      )}
    </div>
  );
}

/** Bringing files onto this machine from Compare's folders and detail pane. */
interface BringHere {
  /** The file a row's product can bring, if it can come. */
  file: (productId: string) => FileToBring | undefined;
  needs: Needs;
  /** The machine files come from; undefined for the Primary Rig. */
  from: string | undefined;
  busy: boolean;
  /** "Bring over 12" or "Copy 12 here". */
  label: (n: number) => string;
  hint: string;
  bring: (files: FileToBring[], tooLarge?: number) => void;
  message: { text: string; warn: string | null } | null;
  /** Every Reaper script (and package file) that can come, and how many can't (too large). */
  scripts: { files: FileToBring[]; tooLarge: number };
  /** The same for every template. */
  templates: { files: FileToBring[]; tooLarge: number };
}

/**
 * Looking at this machine, files come from the Primary Rig; looking at
 * another linked machine, they're copied from it. Either way each file comes
 * with every file it needs (decision #62).
 */
function useBringHere(machine: MachineCard | null, refresh: unknown, onError: (e: string) => void): BringHere {
  const from = machine && !machine.is_this_machine ? machine.id : undefined;
  const [view, setView] = useState<BringView | null>(null);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<BringHere["message"]>(null);
  const id = machine?.id;
  useEffect(() => {
    setMessage(null);
  }, [id]);
  useEffect(() => {
    if (!id) return;
    let current = true;
    filesToBring(from).then(
      (v) => current && setView(v),
      () => current && setView(null),
    );
    return () => {
      current = false;
    };
  }, [id, from, refresh]);
  const byProduct = useMemo(
    () =>
      new Map(
        view?.linked ? view.files.filter((f) => f.product_id && !f.incomplete).map((f) => [f.product_id!, f]) : [],
      ),
    [view],
  );
  const needs = useMemo(() => new Needs(view?.files ?? []), [view]);
  const catchAll = useCallback(
    (which: (f: FileToBring) => boolean) => {
      const all = view?.linked ? view.files.filter(which) : [];
      return {
        files: all.filter((f) => !f.incomplete && !isConflictCopy(f.rel)),
        tooLarge: all.filter((f) => f.incomplete).length,
      };
    },
    [view],
  );
  const scripts = useMemo(() => catchAll(isReaperScript), [catchAll]);
  const templates = useMemo(() => catchAll(isTemplate), [catchAll]);
  const source = from ? (machine?.name ?? "that machine") : "the Primary machine";
  return {
    file: (p) => byProduct.get(p),
    needs,
    from,
    busy,
    label: (n) => (from ? `Copy ${n} here` : `Bring over ${n}`),
    hint: from
      ? `Copies ${machine?.name}'s files onto this machine. Anything replaced is saved first; undo in History.`
      : "Brings the Primary Rig's copy onto this machine. Anything replaced is saved first; undo in History.",
    bring: (files, tooLarge = 0) => {
      setBusy(true);
      setMessage(null);
      bringFiles(files.map(fileKey), from)
        .then((r) => setMessage(bringSummary(r, source, tooLarge)))
        .catch((e) => onError(String(e)))
        .finally(() => setBusy(false));
    },
    message,
    scripts,
    templates,
  };
}

/** A folder's "Bring over 12" / "Copy 12 here": the rows in it this machine can get. */
function BringButton({ rows, bring }: { rows: ComparisonRow[]; bring: BringHere }) {
  const files = rows.map((r) => bring.file(r.product_id)).filter((f): f is FileToBring => f !== undefined);
  if (files.length === 0) return null;
  const along = bring.needs.withNeeds(files.map(fileKey)).size - files.length;
  return (
    <Button
      variant="link"
      disabled={bring.busy}
      onClick={() => bring.bring(files)}
      className="h-6 shrink-0 px-2"
      title={`${along > 0 ? `Also brings the ${along} file${along === 1 ? "" : "s"} they need. ` : ""}${bring.hint}`}
    >
      {bring.label(files.length)}
    </Button>
  );
}

function PlanView({
  id,
  name,
  here,
  refresh,
  onError,
  onBack,
}: {
  id: string;
  name: string;
  /** The plan is for this machine, so its apps and installers can be opened. */
  here: boolean;
  /** Changes after every rescan, to reload the ticks. */
  refresh: unknown;
  onError: (e: string) => void;
  onBack: () => void;
}) {
  const [progress, setProgress] = useState<SetupProgress | null>(null);
  const quick = useQuickSteps(progress?.steps ?? NO_STEPS);
  const [apps, setApps] = useState<VendorApp[]>([]);
  const [installers, setInstallers] = useState<Map<string, VaultMatch>>(new Map());
  useEffect(() => {
    let current = true;
    planProgress(id).then(
      (p) => current && setProgress(p),
      (e) => current && onError(String(e)),
    );
    return () => {
      current = false;
    };
  }, [id, refresh, onError]);
  const wanted = progress?.steps.flatMap((s) => s.open_products).join("\n") ?? "";
  useEffect(() => {
    if (!here) return;
    vendorApps().then(setApps, () => {});
    const products = wanted ? wanted.split("\n") : [];
    vaultMatches(products).then(
      (m) => setInstallers(new Map(m.map((x) => [x.product_id, x]))),
      () => {},
    );
  }, [here, wanted]);
  const actions: Actions = {
    app: (app) => (here ? apps.find((a) => a.id === app) : undefined),
    installer: (product) => (here ? installers.get(product) : undefined),
    open: (app) => openVendorApp(app).catch((e) => onError(String(e))),
    run: (entry) => vaultRun(entry).catch((e) => onError(String(e))),
    onError,
    // This machine's own checklist does the work inside its steps, as Set up does.
    bringOver: here,
    ignore: (label, products) =>
      quick.hide(products, ignoreItems(id, label, productRules(products))).catch((e) => onError(String(e))),
  };
  const ignores = useIgnores(id, refresh);
  const tick = (key: string, done: boolean) =>
    quick.tick(
      key,
      done,
      planTick(id, key, done).then(setProgress, (e) => {
        onError(String(e));
        throw e;
      }),
    );
  const steps = quick.steps;
  return (
    <div className="flex flex-col gap-3 px-5 py-4">
      <header className="flex items-center gap-4">
        <div className="min-w-0 flex-1">
          <h1 className="truncate text-sm font-semibold">
            {here ? `Up To Speed: ${name}` : `${name}'s checklist`}
          </h1>
          <p className="truncate text-xs text-dim">
            {here
              ? "Work down the list. Steps tick themselves off as Patch Pilot sees them done."
              : `To work through it, open Patch Pilot on ${name}. Steps tick themselves off as ${name} finds them done.`}
          </p>
        </div>
        {progress && (
          <span className="text-xs tabular-nums text-dim">
            {steps.filter((s) => s.status === "done" || s.status === "skipped").length} of {progress.total}
          </span>
        )}
        <Button variant="quiet" onClick={onBack} className="shrink-0 whitespace-nowrap">
          Back to Compare
        </Button>
      </header>
      {progress && steps.length === 0 && <p className="text-dim">Nothing to do.</p>}
      {progress?.complete && steps.length > 0 && (
        <p className="text-xs font-medium text-ok">Every step is done.</p>
      )}
      <StepList steps={steps} scope={`plan.${id}`} onTick={tick} actions={actions} />
      {progress && <OtherFormatsNote items={progress.other_formats} />}
      {here && progress && <StepExtras steps={steps} onError={onError} />}
      <IgnoredToggle ignores={ignores} machine={here ? null : id} onError={onError} />
    </div>
  );
}

/** "Open Native Access", or "Get Native Access" when it isn't installed here. */
export function AppButton({ app, onOpen }: { app: VendorApp | undefined; onOpen: (id: string) => void }) {
  if (!app || (!app.can_open && !app.download_url)) return null;
  return (
    <Button
      variant="quiet"
      className="shrink-0 self-start"
      title={app.can_open ? undefined : "Opens the vendor's download page"}
      onClick={(e) => {
        e.preventDefault();
        onOpen(app.id);
      }}
    >
      {app.can_open ? "Open" : "Get"} {app.name}
    </Button>
  );
}
