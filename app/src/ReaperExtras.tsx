import { useCallback, useEffect, useState } from "react";
import { fixReaperFolders, reaperExtras, restoreReapack, type ReaperExtras as Extras } from "./api";
import { Badge, Button, Folder, useFolders } from "./ui";

/**
 * Reaper extras (decision #52): the Golden Rig's ReaPack repositories and
 * packages, and Reaper's folder settings pointed at this machine's folders.
 * Each is one click; Reaper must be closed, and every change is a restore
 * point that History can undo. Loads its own data, like Bring over.
 */
export function ReaperExtras({ onError }: { onError: (e: string) => void }) {
  const [extras, setExtras] = useState<Extras | null>(null);
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState<"reapack" | "folders" | null>(null);
  const [done, setDone] = useState<string | null>(null);
  const folders = useFolders("reaper-extras");

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
          <div className="-mx-1 mt-1 text-xs">
            {remotes.length > 0 && (
              <Folder
                title="Repositories"
                count={remotes.length}
                className="px-1"
                open={folders.isOpen("remotes")}
                onToggle={() => folders.toggle("remotes")}
              >
                <ul className="flex flex-col pl-5 pr-1">
                  {remotes.map((r) => (
                    <li key={r.url} className="truncate py-0.5" title={r.url}>
                      {r.name}
                    </li>
                  ))}
                </ul>
              </Folder>
            )}
            {byRemote(packages).map(([remote, ps]) => (
              <Folder
                key={remote}
                title={remote}
                count={ps.length}
                className="px-1"
                open={folders.isOpen(`packages/${remote}`)}
                onToggle={() => folders.toggle(`packages/${remote}`)}
              >
                <ul className="flex flex-col pl-5 pr-1">
                  {ps.map((p) => (
                    <li key={`${p.remote}/${p.category}/${p.package}`} className="flex items-center gap-2 py-0.5">
                      <span className="min-w-0 flex-1 truncate" title={`${p.remote} · ${p.category}`}>
                        {p.desc || p.package}
                        <span className="ml-1.5 text-[11px] text-dim">{p.category}</span>
                      </span>
                      <span className="text-[11px] tabular-nums text-dim">{p.version}</span>
                    </li>
                  ))}
                </ul>
              </Folder>
            ))}
          </div>
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

/** Packages grouped by the repository they come from, in first-seen order. */
function byRemote<P extends { remote: string }>(packages: P[]): [string, P[]][] {
  const m = new Map<string, P[]>();
  for (const p of packages) m.set(p.remote, [...(m.get(p.remote) ?? []), p]);
  return [...m.entries()];
}

function count(n: number, one: string, many = `${one}s`): string {
  return `${n} ${n === 1 ? one : many}`;
}
