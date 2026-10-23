import { useEffect, useState } from "react";
import {
  makeGolden,
  onScanProgress,
  overview as loadOverview,
  scanMachine,
  type Overview,
  type Progress,
} from "./api";
import { Rig } from "./screens/Rig";
import { Scanning } from "./screens/Scanning";
import { Welcome } from "./screens/Welcome";

export default function App() {
  const [overview, setOverview] = useState<Overview | null>(null);
  const [progress, setProgress] = useState<Progress | null>(null);
  const [scanning, setScanning] = useState(false);
  const [busy, setBusy] = useState(false);
  const [warnings, setWarnings] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    loadOverview().then(setOverview, (e) => setError(String(e)));
    // Event-driven, no polling: nothing runs while the app sits idle.
    const unlisten = onScanProgress(setProgress);
    return () => {
      unlisten.then((f) => f());
    };
  }, []);

  async function scan(name: string | null) {
    setError(null);
    setProgress(null);
    setScanning(true);
    try {
      const report = await scanMachine(name);
      setOverview(report.overview);
      setWarnings(report.warnings);
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
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <main className="min-h-screen bg-white text-neutral-900 antialiased dark:bg-neutral-950 dark:text-neutral-100">
      {error && (
        <div className="border-b border-red-200 bg-red-50 px-6 py-2 text-sm text-red-800 dark:border-red-900/60 dark:bg-red-950/40 dark:text-red-300">
          {error}
        </div>
      )}
      {overview === null ? null : scanning ? (
        <Scanning progress={progress} />
      ) : overview.machine ? (
        <Rig
          overview={overview}
          warnings={warnings}
          busy={busy}
          onRescan={() => scan(null)}
          onMakeGolden={golden}
        />
      ) : (
        <Welcome onScan={(name) => scan(name)} />
      )}
    </main>
  );
}
