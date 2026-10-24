import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import {
  importBundle,
  machines as loadMachines,
  makeGolden,
  onScanProgress,
  onSyncStatus,
  onVaultChanged,
  overview as loadOverview,
  scanMachine,
  setupView as loadSetupView,
  syncStatus as loadSyncStatus,
  vaultStatus,
  type MachinesView,
  type Overview,
  type Progress,
  type SetupView,
  type SyncStatus,
} from "./api";
import { LinkDialog, SyncBadge } from "./Link";
import { ReportDialog } from "./Report";
import { Compare, type Focus } from "./screens/Compare";
import { History } from "./screens/History";
import { Rig } from "./screens/Rig";
import { Vault } from "./screens/Vault";
import { Scanning } from "./screens/Scanning";
import { Setup, SetupOffer } from "./screens/Setup";
import { Welcome } from "./screens/Welcome";
import { resetTour, Tour, tourPending } from "./Tour";
import { Mark } from "./ui";
import { UpdateBar } from "./Update";

type Tab = "rig" | "compare" | "history" | "vault" | "setup";

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
  const [focus, setFocus] = useState<Focus | null>(null);
  const [tour, setTour] = useState(tourPending);
  // "Replay First Run" in the tray menu (temporary, for testing onboarding):
  // the welcome screen and tour as a new user sees them. Saved data is untouched;
  // scanning from there just rescans this machine.
  const [replay, setReplay] = useState(false);
  const [sync, setSync] = useState<SyncStatus | null>(null);
  const [linking, setLinking] = useState(false);
  const [setup, setSetup] = useState<SetupView | null>(null);
  // Bumped on every refresh so History reloads with the rest.
  const [dataVersion, setDataVersion] = useState(0);
  const [dropped, setDropped] = useState<{ paths: string[]; at: number } | null>(null);
  const [vaultCount, setVaultCount] = useState(0);
  const [reporting, setReporting] = useState(false);

  const refresh = useCallback(() => {
    loadOverview().then(setOverview, (e) => setError(String(e)));
    loadMachines().then(setView, (e) => setError(String(e)));
    loadSetupView().then(setSetup, (e) => setError(String(e)));
    setDataVersion((v) => v + 1);
  }, []);
  const onError = useCallback((e: string) => setError(e), []);
  const onNotice = useCallback((lines: string[]) => setNotice(lines), []);

  useEffect(() => {
    refresh();
    // Event-driven, no polling: nothing runs while the app sits idle.
    const unlistenProgress = onScanProgress(setProgress);
    loadSyncStatus().then(setSync, () => {});
    const unlistenSync = onSyncStatus(setSync);
    // Drop a `patchpilot export` file anywhere to add that machine.
    const unlistenDrop = getCurrentWebview().onDragDropEvent(async (e) => {
      if (e.payload.type !== "drop") return;
      // Anything else (installers, a folder) goes to the Installer Vault.
      const others = e.payload.paths.filter((p) => !p.toLowerCase().endsWith(".json"));
      if (others.length > 0) {
        setDropped({ paths: others, at: Date.now() });
        setTab("vault");
      }
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
    // The tray popover's Expand / Get Up To Speed buttons land here.
    const unlistenShow = listen<{ id: string; plan: boolean }>("show-machine", (e) => {
      setFocus({ ...e.payload, at: Date.now() });
      setTab("compare");
    });
    // A rescan from the tray popover.
    const unlistenChanged = listen("data-changed", refresh);
    // The Vault tab's badge: installers waiting in Downloads.
    const vaultCount = () => vaultStatus().then((v) => setVaultCount(v.inbox.length), () => {});
    vaultCount();
    const unlistenVault = onVaultChanged(vaultCount);
    const unlistenReplay = listen("replay-first-run", () => {
      resetTour();
      setReplay(true);
      setTour(true);
      setError(null);
      setNotice(null);
    });
    return () => {
      unlistenShow.then((f) => f());
      unlistenChanged.then((f) => f());
      unlistenVault.then((f) => f());
      unlistenReplay.then((f) => f());
      unlistenProgress.then((f) => f());
      unlistenSync.then((f) => f());
      unlistenDrop.then((f) => f());
    };
  }, [refresh]);

  async function scan(name: string | null) {
    setError(null);
    setProgress(null);
    setScanning(true);
    setReplay(false);
    try {
      const report = await scanMachine(name);
      setOverview(report.overview);
      setWarnings(report.warnings);
      setDataVersion((v) => v + 1);
      loadMachines().then(setView, (e) => setError(String(e)));
      loadSetupView().then(setSetup, (e) => setError(String(e)));
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
      {overview?.machine && !scanning && !replay && (
        <nav className="flex h-10 shrink-0 items-center gap-4 border-b border-line bg-panel px-3">
          <span className="flex items-center gap-1.5 font-semibold">
            <Mark className="text-accent-text" />
            Patch Pilot
          </span>
          {(
            [
              ["rig", "This machine"],
              ["compare", "Compare"],
              ["vault", "Vault"],
              ["history", "History"],
              ...(setup?.active || tab === "setup" ? ([["setup", "Set up"]] as const) : []),
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
              {t === "vault" && vaultCount > 0 && (
                <span className="ml-1 rounded bg-accent px-1 text-[10px] font-semibold text-on-accent">{vaultCount}</span>
              )}
            </button>
          ))}
          <span className="flex-1" />
          <SyncBadge status={sync} onClick={() => setLinking(true)} />
          <button onClick={() => setTour(true)} className="text-dim hover:text-fg">
            How it works
          </button>
        </nav>
      )}
      {replay && (
        <div className="flex h-8 shrink-0 items-center gap-3 border-b border-line bg-panel px-3 text-xs text-dim">
          <span className="flex-1">Replaying the first run. Your saved machines and Golden Rig are untouched.</span>
          <button onClick={() => setReplay(false)} className="hover:text-fg">
            Exit
          </button>
        </div>
      )}
      <UpdateBar />
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
        ) : !overview.machine || replay ? (
          <Welcome onScan={(name) => scan(name)} onTour={() => setTour(true)} />
        ) : tab === "setup" ? (
          <Setup onRescan={() => scan(null)} onError={onError} onChange={setSetup} />
        ) : tab === "vault" ? (
          <Vault dropped={dropped} onError={onError} onNotice={onNotice} onCount={setVaultCount} />
        ) : tab === "history" ? (
          <History refreshKey={dataVersion} onError={onError} />
        ) : tab === "compare" && view && canCompare ? (
          <Compare view={view} focus={focus} onError={onError} />
        ) : (
          <>
            {setup?.offer && <SetupOffer onOpen={() => setTab("setup")} />}
            <Rig
              overview={overview}
              warnings={warnings}
              busy={busy}
              onRescan={() => scan(null)}
              onMakeGolden={golden}
              onReport={() => setReporting(true)}
            />
          </>
        )}
      </div>
      {tour && overview !== null && !scanning && <Tour onClose={() => setTour(false)} />}
      {linking && <LinkDialog status={sync} onClose={() => setLinking(false)} />}
      {reporting && overview?.machine && (
        <ReportDialog
          machineName={overview.machine.name}
          hasGolden={!!overview.golden}
          onClose={() => setReporting(false)}
        />
      )}
    </main>
  );
}
