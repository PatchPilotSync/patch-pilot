import { useCallback, useEffect, useState } from "react";
import { fixReaperFolders, reaperExtras, restoreReapack, type ReaperExtras as Extras } from "./api";
import { Badge, Button } from "./ui";

/**
 * Reaper extras (decision #48): the Golden Rig's ReaPack repositories and
 * packages, and Reaper's folder settings pointed at this machine's folders.
 * Each is one click; Reaper must be closed, and every change is a restore
 * point that History can undo. Loads its own data, like Bring over.
 */
export function ReaperExtras({ onError }: { onError: (e: string) => void }) {
  const [extras, setExtras] = useState<Extras | null>(null);
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState<"reapack" | "folders" | null>(null);
  const [done, setDone] = useState<string | null>(null);

  const load = useCallback(() => {
    reaperExtras().then(setExtras, (e) => onError(String(e)));
  }, [onError]);
  useEffect(load, [load]);

  if (!extras || !extras.reaper_found) return null;
  const { missing_remotes: remotes, missing_packages: packages } = extras.reapack;
  const reapackCount = remotes.length + packages.length;
  const fixable = extras.folders.filter((f) => f.proposed !== null);
  const unmatched = extras.folders.filter((f) => f.proposed === null);
  if (reapackCount === 0 && extras.folders.length === 0 && !done) return null;

  async function run(what: "reapack" | "folders") {
    setBusy(what);
    setDone(null);
    try {
      setExtras(await (what === "reapack" ? restoreReapack() : fixReaperFolders()));
      setDone(
        what === "reapack"
          ? "Done. In Reaper, choose Extensions > ReaPack > Synchronize packages to install them."
          : "Done. Reaper uses the new folders next time it opens.",
      );
    } catch (e) {
      onError(String(e));
    } finally {
      setBusy(null);
    }
  }

  const parts = [
    reapackCount > 0 && `ReaPack: ${count(packages.length, "package")}${remotes.length ? `, ${count(remotes.length, "repository", "repositories")}` : ""}`,
    extras.folders.length > 0 && `Folders: ${count(extras.folders.length, "setting")}`,
  ].filter(Boolean);

  return (
    <section className="rounded-lg border border-line bg-panel px-3 py-2.5">
      <header className="flex items-center gap-3">
        <div className="min-w-0 flex-1">
          <h2 className="text-xs font-semibold">Reaper setup</h2>
          <p className="truncate text-[11px] text-dim">{parts.length ? parts.join(" · ") : "Matches the Golden Rig."}</p>
        </div>
        {parts.length > 0 && (
          <Button variant="quiet" onClick={() => setOpen((o) => !o)} className="shrink-0">
            {open ? "Hide" : "Show"}
          </Button>
        )}
      </header>

      {done && <p className="mt-1.5 text-[11px] text-ok">{done}</p>}

      {open && reapackCount > 0 && (
        <div className="mt-2">
          <div className="flex items-center gap-3">
            <h3 className="flex-1 text-[11px] font-medium text-dim">ReaPack</h3>
            {extras.reapack_installed && (
              <Button onClick={() => run("reapack")} disabled={busy !== null} className="shrink-0 whitespace-nowrap">
                {busy === "reapack" ? "Restoring…" : `Restore ${reapackCount}`}
              </Button>
            )}
          </div>
          {!extras.reapack_installed && (
            <p className="text-[11px] text-warn">Install ReaPack in Reaper first (reapack.com), then rescan.</p>
          )}
          <ul className="mt-1 flex flex-col text-xs">
            {remotes.map((r) => (
              <li key={r.url} className="flex items-center gap-2 py-0.5">
                <span className="min-w-0 flex-1 truncate" title={r.url}>
                  {r.name}
                </span>
                <Badge>Repository</Badge>
              </li>
            ))}
            {packages.map((p) => (
              <li key={`${p.remote}/${p.category}/${p.package}`} className="flex items-center gap-2 py-0.5">
                <span className="min-w-0 flex-1 truncate" title={`${p.remote} · ${p.category}`}>
                  {p.desc || p.package}
                  <span className="ml-1.5 text-[11px] text-dim">{p.remote}</span>
                </span>
                <span className="text-[11px] tabular-nums text-dim">{p.version}</span>
              </li>
            ))}
          </ul>
          <p className="mt-1 text-[11px] text-dim">
            Patch Pilot adds the repositories and marks the packages for ReaPack, which downloads them when you choose
            Synchronize packages. Reaper must be closed.
          </p>
        </div>
      )}

      {open && extras.folders.length > 0 && (
        <div className="mt-2">
          <div className="flex items-center gap-3">
            <h3 className="flex-1 text-[11px] font-medium text-dim">Folders</h3>
            {fixable.length > 0 && (
              <Button onClick={() => run("folders")} disabled={busy !== null} className="shrink-0 whitespace-nowrap">
                {busy === "folders" ? "Setting…" : `Set ${fixable.length}`}
              </Button>
            )}
          </div>
          <ul className="mt-1 flex flex-col text-xs">
            {[...fixable, ...unmatched].map((f) => (
              <li key={f.name} className="py-0.5">
                <div className="flex items-center gap-2">
                  <span className="min-w-0 flex-1 truncate">{f.name}</span>
                  {f.proposed === null && <Badge tone="amber">Not found here</Badge>}
                </div>
                <div className="truncate text-[11px] text-dim" title={f.proposed ?? f.golden}>
                  {f.proposed !== null
                    ? `${f.current ?? "Not set"} → ${f.proposed}`
                    : `Golden Rig uses ${f.golden}`}
                </div>
              </li>
            ))}
          </ul>
          <p className="mt-1 text-[11px] text-dim">
            Matched by the same folder names on this machine's drives. Reaper must be closed; you can undo it in History.
          </p>
        </div>
      )}
    </section>
  );
}

function count(n: number, one: string, many = `${one}s`): string {
  return `${n} ${n === 1 ? one : many}`;
}
