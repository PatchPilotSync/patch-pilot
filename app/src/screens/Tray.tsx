import { useCallback, useEffect, useState, type ReactNode } from "react";
import { emit, listen } from "@tauri-apps/api/event";
import {
  compareMachine,
  FORMAT_LABEL,
  machines as loadMachines,
  openMainWindow,
  overview as loadOverview,
  scanMachine,
  type ComparisonRow,
  type MachineCard,
  type MachineComparison,
  type MachinesView,
  type Overview,
} from "../api";
import { Button, Mark, scoreColor, timeAgo } from "../ui";

// The tray / menu bar popover (decision #35): the compact view of every
// machine's parity. It loads when it is shown and never polls.

const SHOWN = 5;
const WORK = ["missing", "incomplete", "outdated"] as const;

export function Tray() {
  const [overview, setOverview] = useState<Overview | null>(null);
  const [view, setView] = useState<MachinesView | null>(null);
  const [target, setTarget] = useState<MachineComparison | null>(null);
  const [scanning, setScanning] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      const [o, v] = await Promise.all([loadOverview(), loadMachines()]);
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
    return () => {
      unlisten.then((f) => f());
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

  return (
    <div className="flex h-full flex-col overflow-hidden border border-line bg-panel">
      <header className="flex h-10 shrink-0 items-center gap-2 px-3.5">
        <Mark className="text-accent-text" />
        <span className="font-semibold">Patch Pilot</span>
        <span className="flex-1" />
        {machine && (
          <span className="text-[11px] text-dim">{scanning ? "Scanning…" : `Scanned ${timeAgo(machine.scanned_at)}`}</span>
        )}
      </header>

      <div className="flex min-h-0 flex-1 flex-col gap-2 overflow-auto px-3 pb-3">
        {error && <p className="text-xs text-bad">{error}</p>}

        {overview && !machine && <Hint text="Scan this machine to get started." />}
        {machine && view?.golden_version === null && <Hint text="Make a Golden Rig to compare your machines." />}

        {view && view.golden_version !== null && (
          <h2 className="px-1 text-[11px] text-dim">Parity with Golden Rig v{view.golden_version}</h2>
        )}
        {view && view.golden_version !== null && (
          <ul className="flex flex-col gap-0.5">
            {view.machines.map((m) => (
              <MachineRow
                key={m.id}
                m={m}
                active={m.id === target?.machine.id}
                onOpen={() => openMainWindow(m.id)}
              />
            ))}
          </ul>
        )}

        {target && (
          <section className="rounded-md border border-line bg-canvas py-1">
            {todo.length === 0 ? (
              <p className="px-3 py-1.5 text-xs text-ok">{target.machine.name} is at parity with the Golden Rig.</p>
            ) : (
              <>
                <h2 className="px-3 pb-0.5 pt-1 text-[11px] text-dim">{target.machine.name} needs</h2>
                <ul>
                  {todo.slice(0, SHOWN).map((r) => (
                    <NeedRow key={r.product_id} r={r} />
                  ))}
                </ul>
                {todo.length > SHOWN && (
                  <button
                    onClick={() => openMainWindow(target.machine.id)}
                    className="px-3 py-1 text-[11px] text-dim hover:text-fg"
                  >
                    and {todo.length - SHOWN} more
                  </button>
                )}
              </>
            )}
          </section>
        )}
      </div>

      <footer className="flex h-12 shrink-0 items-center gap-1 border-t border-line px-2.5">
        <IconButton label="Rescan this machine" onClick={rescan} disabled={scanning || !machine}>
          <path d="M20 11a8 8 0 1 0-2.3 5.7" />
          <path d="M20 4v7h-7" />
        </IconButton>
        <IconButton label="Open the full window" onClick={() => openMainWindow()}>
          <path d="M14 4h6v6M10 20H4v-6M20 4l-7 7M4 20l7-7" />
        </IconButton>
        <span className="flex-1" />
        {target && todo.length > 0 ? (
          <Button onClick={() => openMainWindow(target.machine.id, true)}>Up To Speed</Button>
        ) : (
          <Button onClick={() => openMainWindow()}>Open</Button>
        )}
      </footer>
    </div>
  );
}

/** The machine furthest from Golden; the Golden machine itself only if it's the only one. */
function pickTarget(view: MachinesView): MachineCard | null {
  const scored = [...view.machines].sort((a, b) => (a.score ?? 101) - (b.score ?? 101));
  return scored[0] ?? null;
}

function MachineRow({ m, active, onOpen }: { m: MachineCard; active: boolean; onOpen: () => void }) {
  return (
    <li>
      <button
        onClick={onOpen}
        className={`flex h-12 w-full items-center gap-3 rounded-md border px-3 text-left ${
          active ? "border-line bg-canvas" : "border-transparent hover:bg-sel/50"
        }`}
      >
        <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" className="shrink-0 text-dim" aria-hidden="true">
          <rect x="3" y="4" width="18" height="12" rx="1.5" />
          <path d="M8 20h8M12 16v4" />
        </svg>
        <span className="min-w-0 flex-1">
          <span className="block truncate font-semibold">{m.name}</span>
          <span className="block truncate text-[11px] text-dim">
            {m.is_this_machine ? "This machine" : m.os === "macos" ? "macOS" : "Windows"} · {timeAgo(m.scanned_at)}
          </span>
        </span>
        <span className={`text-[15px] font-semibold tabular-nums ${scoreColor(m.score)}`}>{m.score ?? "–"}</span>
        <span className="text-dim" aria-hidden="true">
          ›
        </span>
      </button>
    </li>
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

function NeedRow({ r }: { r: ComparisonRow }) {
  const missing = r.state === "missing";
  const note = missing
    ? KIND_NOTE[r.kind]
    : r.state === "incomplete" && r.missing_formats.length
      ? `no ${r.missing_formats.map((f) => FORMAT_LABEL[f]).join(", ")}`
      : r.state === "outdated" && r.machine_version && r.golden_version
        ? `${r.machine_version} → ${r.golden_version}`
        : r.state;
  return (
    <li className="flex h-7 items-center gap-2.5 px-3">
      <span className={`w-3 shrink-0 text-center text-[13px] font-semibold ${missing ? "text-bad" : "text-warn"}`} aria-hidden="true">
        {missing ? "✕" : "!"}
      </span>
      <span className="min-w-0 flex-1 truncate">{r.name}</span>
      <span className="shrink-0 text-[11px] text-dim">{note}</span>
    </li>
  );
}

function Hint({ text }: { text: string }) {
  return <p className="rounded-md border border-line bg-canvas px-3 py-2.5 text-xs text-dim">{text}</p>;
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
