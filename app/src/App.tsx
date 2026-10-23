import { useEffect, useState } from "react";
import { appVersion, type AppVersion } from "./api";

export default function App() {
  const [version, setVersion] = useState<AppVersion | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    appVersion().then(setVersion, (e) => setError(String(e)));
  }, []);

  return (
    <main className="flex min-h-screen flex-col items-center justify-center gap-2 bg-white text-neutral-900 dark:bg-neutral-950 dark:text-neutral-100">
      <h1 className="text-2xl font-semibold tracking-tight">Patch Pilot</h1>
      <p className="text-sm text-neutral-500">Every rig, ready.</p>
      <p className="mt-6 font-mono text-xs text-neutral-400">
        {version
          ? `v${version.app} · manifest schema v${version.manifest_schema}`
          : (error ?? "Connecting…")}
      </p>
    </main>
  );
}
