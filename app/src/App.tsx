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
    <main className="min-h-screen bg-white text-neutral-900 antialiased dark:bg-neutral-950 dark:text-neutral-100">
      {overview?.machine && !scanning && (
        <nav className="flex h-12 items-center gap-1 border-b border-neutral-200 px-4 text-sm dark:border-neutral-800">
          <span className="mr-4 font-semibold tracking-tight">Patch Pilot</span>
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
              className={`rounded-md px-3 py-1.5 disabled:opacity-40 ${
                tab === t ? "bg-neutral-100 font-medium dark:bg-neutral-800" : "text-neutral-500"
              }`}
            >
              {label}
            </button>
          ))}
        </nav>
      )}
      {error && (
        <div className="border-b border-red-200 bg-red-50 px-6 py-2 text-sm text-red-800 dark:border-red-900/60 dark:bg-red-950/40 dark:text-red-300">
          {error}
        </div>
      )}
      {notice && (
        <div className="flex items-start gap-4 border-b border-emerald-200 bg-emerald-50 px-6 py-2 text-sm text-emerald-900 dark:border-emerald-900/60 dark:bg-emerald-950/40 dark:text-emerald-200">
          <ul className="flex-1">
            {notice.map((l) => (
              <li key={l}>{l}</li>
            ))}
          </ul>
          <button onClick={() => setNotice(null)} className="text-xs opacity-70">
            Dismiss
          </button>
        </div>
      )}
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
    </main>
  );
}
