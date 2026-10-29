import { useCallback, useEffect, useState, type MouseEvent, type ReactNode } from "react";
import { emit, listen } from "@tauri-apps/api/event";
import {
  compareMachine,
  FORMAT_LABEL,
  groupSummary,
  machines as loadMachines,
  openMainWindow,
  overview as loadOverview,
  reaperFileLabel,
  onUpdateScan,
  scanMachine,
  updateScanRunning,
  updates as loadUpdates,
  type ComparisonRow,
  type MachineCard,
  type MachineComparison,
  type MachinesView,
  type Overview,
} from "../api";
import { DiskSummary } from "../Disks";
import { Button, Glyph, Loading, Mark, Panel, Tile, scoreColor, scoreTone, timeAgo, type GlyphName } from "../ui";
import { STATE_TILE } from "./Compare";

// The tray / menu bar popover (decision #35): the compact view of every
// machine's parity. It loads when it is shown and never polls. Every route
// into the main window is visible (decision #37): a labelled Expand button,
// clickable machine and need rows, and a double-click on any empty spot.

const SHOWN = 3;
const WORK = ["missing", "incomplete", "outdated"] as const;

export function Tray() {
  const [overview, setOverview] = useState<Overview | null>(null);
  const [view, setView] = useState<MachinesView | null>(null);
  const [target, setTarget] = useState<MachineComparison | null>(null);
  const [scanning, setScanning] = useState(false);
  const [updateCount, setUpdateCount] = useState(0);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      const [o, v, u] = await Promise.all([loadOverview(), loadMachines(), loadUpdates()]);
      setUpdateCount(u.alerts.length);
      setOverview(o);
      setView(v);
      const worst = pickTarget(v);
      setTarget(worst && v.golden_version !== null ? await compareMachine(worst.id) : null);
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  }, []);

  useEffect(() => {
    load();
    const unlisten = listen("popover-shown", load);
    // The rescan on the first launch after an update shows here too.
    updateScanRunning().then(setScanning, () => {});
    const unlistenUpdateScan = onUpdateScan((running) => {
      setScanning(running);
      if (!running) load();
    });
    return () => {
      unlisten.then((f) => f());
      unlistenUpdateScan.then((f) => f());
    };
  }, [load]);

  async function rescan() {
    setScanning(true);
    try {
      await scanMachine(null);
      await emit("data-changed");
      await load();
    } catch (e) {
      setError(String(e));
    } finally {
      setScanning(false);
    }
  }

  const machine = overview?.machine;
  const todo = target ? target.comparison.rows.filter((r) => (WORK as readonly string[]).includes(r.state)) : [];
  // Missing first, and within that licensing and vendor apps, which other installs depend on.
  const rank = (r: ComparisonRow) =>
    WORK.indexOf(r.state as (typeof WORK)[number]) * 10 + KIND_ORDER.indexOf(r.kind);
  todo.sort((a, b) => rank(a) - rank(b));
  const behind = target && todo.length > 0 ? target.machine : null;
  // "Propose to Primary Rig": this machine first, then whoever has the most.
  const suggesting =
    view?.machines
      .filter((m) => m.suggestions > 0)
      .sort((a, b) => Number(b.is_this_machine) - Number(a.is_this_machine) || b.suggestions - a.suggestions)[0] ?? null;

  // Double-click on anything that isn't a control opens the main window,
  // on the machine that needs work if there is one.
  function onDoubleClick(e: MouseEvent<HTMLDivElement>) {
    if ((e.target as Element).closest("button, a, input")) return;
    openMainWindow(behind?.id ?? null);
  }

  return (
    <div
      onDoubleClick={onDoubleClick}
      className="flex h-full cursor-default select-none flex-col overflow-hidden border border-line bg-canvas"
    >
      <header className="flex h-10 shrink-0 items-center gap-2 border-b border-line bg-panel px-3.5">
        <Mark className="text-accent-text" />
        <span className="font-semibold">Patch Pilot</span>
        <span className="flex-1" />
        {machine && (
          <span className="text-[11px] text-dim">{scanning ? "Scanning…" : `Scanned ${timeAgo(machine.scanned_at)}`}</span>
        )}
      </header>

      <div className="flex min-h-0 flex-1 flex-col gap-2 overflow-auto px-2.5 pb-3 pt-2.5">
        {error && <Hint icon="info" tone="bad" text={<span className="text-bad">{error}</span>} />}
        {!overview && !error && <Loading text="Loading your rig…" compact />}

        {overview && !machine && <Hint icon="search" text="Scan this machine to get started." />}
        {machine && view?.golden_version === null && <Hint icon="star" text="Make a Primary Rig to compare your machines." />}

        {view && view.golden_version !== null && (
          <h2 className="-mb-1 px-1 text-[11px] text-dim">Parity with Primary Rig v{view.golden_version}</h2>
        )}
        {view && view.golden_version !== null && (
          <Panel divided>
            {view.machines.map((m) => (
              <MachineRow
                key={m.id}
                m={m}
                active={m.id === target?.machine.id}
                onOpen={() => openMainWindow(m.id)}
              />
            ))}
          </Panel>
        )}

        {(suggesting || updateCount > 0) && (
          <Panel divided>
            {suggesting && (
              <Line
                icon="star"
                onClick={() => openMainWindow(suggesting.id)}
                text={
                  suggesting.golden_source
                    ? `Not in Primary Rig v${view?.golden_version} yet: ${groupSummary(suggesting.suggestion_groups)}`
                    : `For the Primary Rig${suggesting.is_this_machine ? "" : ` from ${suggesting.name}`}: ${groupSummary(suggesting.suggestion_groups)}`
                }
              />
            )}
            {updateCount > 0 && (
              <Line
                icon="download"
                onClick={() => openMainWindow()}
                text={`${updateCount} plug-in update${updateCount === 1 ? "" : "s"} available on this machine`}
              />
            )}
          </Panel>
        )}

        {target && (
          <Panel>
            {todo.length === 0 ? (
              <Line
                icon="check"
                tone="ok"
                onClick={() => openMainWindow(target.machine.id)}
                text={`${target.machine.name} is at parity with the Primary Rig.`}
              />
            ) : (
              <>
                <h2 className="flex h-8 items-center gap-2 border-b border-line px-3 text-[11px] text-dim">
                  <span className="min-w-0 flex-1 truncate">{target.machine.name} needs</span>
                  <span className="tabular-nums">{todo.length}</span>
                </h2>
                <ul className="py-1">
                  {todo.slice(0, SHOWN).map((r) => (
                    <NeedRow key={r.product_id} r={r} onOpen={() => openMainWindow(target.machine.id)} />
                  ))}
                </ul>
                {todo.length > SHOWN && (
                  <button
                    onClick={() => openMainWindow(target.machine.id)}
                    className="h-7 border-t border-line pl-12 pr-3 text-left text-[11px] text-dim hover:bg-sel/50 hover:text-fg"
                  >
                    and {todo.length - SHOWN} more
                  </button>
                )}
              </>
            )}
          </Panel>
        )}

        {machine && !behind && (
          <p className="mt-auto pt-2 text-center text-[11px] text-dim/70">Double-click to open the full window</p>
        )}
      </div>

      <footer className="flex h-12 shrink-0 items-center gap-1 border-t border-line bg-panel px-2.5">
        <IconButton label="Rescan this machine" onClick={rescan} disabled={scanning || !machine}>
          <path d="M20 11a8 8 0 1 0-2.3 5.7" />
          <path d="M20 4v7h-7" />
        </IconButton>
        <span className="flex-1" />
        {behind ? (
          <>
            <Button
              variant={behind.is_this_machine ? "quiet" : "primary"}
              title="Open the full window"
              onClick={() => openMainWindow(behind.id)}
            >
              <ExpandIcon />
              Expand
            </Button>
            {/* Getting another machine up to speed happens on that machine. */}
            {behind.is_this_machine && (
              <Button onClick={() => openMainWindow(behind.id, true)}>Get Up To Speed</Button>
            )}
          </>
        ) : (
          <Button title="Open the full window" onClick={() => openMainWindow()}>
            <ExpandIcon />
            Open Patch Pilot
          </Button>
        )}
      </footer>
    </div>
  );
}

/** The machine furthest from the Primary Rig; the Primary machine itself only if it's the only one. */
function pickTarget(view: MachinesView): MachineCard | null {
  const scored = [...view.machines].sort((a, b) => (a.score ?? 101) - (b.score ?? 101));
  return scored[0] ?? null;
}

function MachineRow({ m, active, onOpen }: { m: MachineCard; active: boolean; onOpen: () => void }) {
  return (
    <button
      onClick={onOpen}
      title={`Open ${m.name} in the full window`}
      className={`flex h-11 w-full items-center gap-2.5 px-2 text-left ${active ? "bg-sel/40" : "hover:bg-sel/50"}`}
    >
      <Tile name="desktop" tone={scoreTone(m.score)} />
      <span className="min-w-0 flex-1">
        <span className="block truncate font-semibold">{m.name}</span>
        <span className="block truncate text-[11px] text-dim">
          {m.is_this_machine ? "This machine" : m.os === "macos" ? "macOS" : "Windows"} · {timeAgo(m.scanned_at)}
          {m.disks.length > 0 && " · "}
          <DiskSummary disks={m.disks} os={m.os} named={false} />
        </span>
      </span>
      <span className={`text-[15px] font-semibold tabular-nums ${scoreColor(m.score)}`}>{m.score ?? "–"}</span>
      <span className="mr-1 text-dim" aria-hidden="true">
        ›
      </span>
    </button>
  );
}

const KIND_ORDER: ComparisonRow["kind"][] = ["license_tool", "vendor_app", "plugin", "daw_asset", "folder", "config"];

const KIND_NOTE: Record<ComparisonRow["kind"], string> = {
  plugin: "plug-in",
  daw_asset: "DAW file",
  vendor_app: "vendor app",
  license_tool: "licensing",
  folder: "folder",
  config: "config",
};

/** Reaper's files by what they are, from their product ID (`reaper-script/...`). */
const REAPER_NOTE: Record<string, string> = {
  "reaper-script": "script",
  "reaper-theme": "theme",
  "reaper-template": "template",
  "reaper-config": "setting",
};

function NeedRow({ r, onOpen }: { r: ComparisonRow; onOpen: () => void }) {
  const missing = r.state === "missing";
  const [glyph, tone] = STATE_TILE[r.state];
  const reaper = REAPER_NOTE[r.product_id.split("/")[0]];
  // A Reaper file's name is its path; the file name says enough here.
  const name = reaper ? reaperFileLabel(r.name) : r.name;
  const note = missing
    ? (reaper ?? KIND_NOTE[r.kind])
    : r.state === "incomplete" && r.missing_formats.length
      ? `no ${r.missing_formats.map((f) => FORMAT_LABEL[f]).join(", ")}`
      : r.state === "outdated" && r.machine_version && r.golden_version
        ? `${r.machine_version} → ${r.golden_version}`
        : r.state;
  return (
    <li>
      <button onClick={onOpen} className="flex h-7 w-full items-center gap-2.5 px-2 pr-3 text-left hover:bg-sel/50">
        <span className={`mx-1 flex w-[22px] shrink-0 justify-center ${tone === "bad" ? "text-bad" : "text-warn"}`} aria-hidden="true">
          <Glyph name={glyph} size={13} />
        </span>
        <span className="min-w-0 flex-1 truncate" title={r.name}>
          {name}
        </span>
        <span className="shrink-0 text-[11px] text-dim">{note}</span>
      </button>
    </li>
  );
}

function ExpandIcon() {
  return (
    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" aria-hidden="true" className="-ml-0.5 mr-1.5">
      <path d="M14 4h6v6M10 20H4v-6M20 4l-7 7M4 20l7-7" />
    </svg>
  );
}

/** A one-line status in its own panel, before there's anything to compare. */
function Hint({ icon, tone = "dim", text }: { icon: GlyphName; tone?: "dim" | "bad"; text: ReactNode }) {
  return (
    <Panel className="p-1">
      <p className="flex min-h-9 items-center gap-2.5 px-1 text-xs text-dim">
        <Tile name={icon} tone={tone} />
        <span className="min-w-0 flex-1">{text}</span>
      </p>
    </Panel>
  );
}

/** A row that opens the full window: a tile, one line and a chevron. */
function Line({
  icon,
  tone = "accent",
  text,
  onClick,
}: {
  icon: GlyphName;
  tone?: "accent" | "ok";
  text: string;
  onClick: () => void;
}) {
  return (
    <button onClick={onClick} className="flex h-9 items-center gap-2.5 px-2 text-left text-xs hover:bg-sel/50">
      <Tile name={icon} tone={tone} />
      <span className={`min-w-0 flex-1 truncate ${tone === "ok" ? "text-ok" : ""}`} title={text}>
        {text}
      </span>
      <span className="mr-1 text-dim" aria-hidden="true">
        ›
      </span>
    </button>
  );
}

function IconButton({
  label,
  onClick,
  disabled,
  children,
}: {
  label: string;
  onClick: () => void;
  disabled?: boolean;
  children: ReactNode;
}) {
  return (
    <button
      aria-label={label}
      title={label}
      onClick={onClick}
      disabled={disabled}
      className="flex h-8 w-8 items-center justify-center rounded-md text-fg hover:bg-sel disabled:opacity-40"
    >
      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" aria-hidden="true">
        {children}
      </svg>
    </button>
  );
}
