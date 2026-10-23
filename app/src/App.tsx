import { useCallback, useEffect, useState } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import {
  importBundle,
  machines as loadMachines,
  makeGolden,
  onScanProgress,
  overview as loadOverview,
  scanMachine,
  type MachinesView,
  type Overview,
  type Progress,
} from "./api";
import { Compare } from "./screens/Compare";
import { Rig } from "./screens/Rig";
import { Scanning } from "./screens/Scanning";
import { Welcome } from "./screens/Welcome";

type Tab = "rig" | "compare";

export default function App() {
  const [overview, setOverview] = useState<Overview | null>(null);
  const [view, setView] = useState<MachinesView | null>(null);
  const [tab, setTab] = useState<Tab>("rig");
  const [progress, setProgress] = useState<Progress | null>(null);
  const [scanning, setScanning] = useState(false);
  const [busy, setBusy] = useState(false);
  const [warnings, setWarnings] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string[] | null>(null);

  const refresh = useCallback(() => {
    loadOverview().then(setOverview, (e) => setError(String(e)));
    loadMachines().then(setView, (e) => setError(String(e)));
  }, []);
  const onError = useCallback((e: string) => setError(e), []);

  useEffect(() => {
    refresh();
    // Event-driven, no polling: nothing runs while the app sits idle.
    const unlistenProgress = onScanProgress(setProgress);
    // Drop a `patchpilot export` file anywhere to add that machine.
    const unlistenDrop = getCurrentWebview().onDragDropEvent(async (e) => {
      if (e.payload.type !== "drop") return;
      for (const path of e.payload.paths.filter((p) => p.toLowerCase().endsWith(".json"))) {
        try {
          setNotice((await importBundle(path)).lines);
          setError(null);
          setTab("compare");
        } catch (err) {
          setError(String(err));
        }
      }
      refresh();
    });
    return () => {
      unlistenProgress.then((f) => f());
      unlistenDrop.then((f) => f());
    };
  }, [refresh]);

  async function scan(name: string | null) {
    setError(null);
    setProgress(null);
    setScanning(true);
    try {
      const report = await scanMachine(name);
      setOverview(report.overview);
      setWarnings(report.warnings);
      loadMachines().then(setView, (e) => setError(String(e)));
    } catch (e) {
      setError(String(e));
    } finally {
      setScanning(false);
    }
  }

  async function golden() {
    setError(null);
    setBusy(true);
    try {
      setOverview(await makeGolden());
      loadMachines().then(setView, (e) => setError(String(e)));
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  const canCompare = !!view && view.golden_version !== null && view.machines.length > 0;

  return (
    <main className="flex h-full flex-col bg-canvas text-fg antialiased">
      {overview?.machine && !scanning && (
        <nav className="flex h-10 shrink-0 items-center gap-4 border-b border-line bg-panel px-3">
          <span className="flex items-center gap-1.5 font-semibold">
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" aria-hidden="true">
              <path d="M4 12h3l2-6 4 12 2-6h5" />
            </svg>
            Patch Pilot
          </span>
          {(
            [
              ["rig", "This machine"],
              ["compare", "Compare"],
            ] as const
          ).map(([t, label]) => (
            <button
              key={t}
              disabled={t === "compare" && !canCompare}
              title={t === "compare" && !canCompare ? "Make a Golden Rig first" : undefined}
              onClick={() => setTab(t)}
              className={`disabled:opacity-40 ${tab === t ? "font-medium text-fg" : "text-dim hover:text-fg"}`}
            >
              {label}
            </button>
          ))}
        </nav>
      )}
      {error && (
        <div className="flex shrink-0 items-start gap-3 border-b border-line bg-bad/10 px-4 py-1.5 text-bad">
          <span className="flex-1">{error}</span>
          <button onClick={() => setError(null)} className="text-dim hover:text-fg">
            Dismiss
          </button>
        </div>
      )}
      {notice && (
        <div className="flex shrink-0 items-start gap-3 border-b border-line bg-ok/10 px-4 py-1.5">
          <ul className="flex-1">
            {notice.map((l) => (
              <li key={l}>{l}</li>
            ))}
          </ul>
          <button onClick={() => setNotice(null)} className="text-dim hover:text-fg">
            Dismiss
          </button>
        </div>
      )}
      <div className="min-h-0 flex-1 overflow-auto">
        {overview === null ? null : scanning ? (
          <Scanning progress={progress} />
        ) : !overview.machine ? (
          <Welcome onScan={(name) => scan(name)} />
        ) : tab === "compare" && view && canCompare ? (
          <Compare view={view} onError={onError} />
        ) : (
          <Rig
            overview={overview}
            warnings={warnings}
            busy={busy}
            onRescan={() => scan(null)}
            onMakeGolden={golden}
          />
        )}
      </div>
    </main>
  );
}
