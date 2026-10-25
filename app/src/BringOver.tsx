import { useCallback, useEffect, useMemo, useState } from "react";
import { bringFiles, fileKey, filesToBring, type BringResult, type BringView, type FileToBring } from "./api";
import { Badge, Button, buildTree, Folder, FolderTree, isConflictCopy, useFolders } from "./ui";

/**
 * Bring over (decision #45): the Golden Rig's templates, scripts and themes
 * this machine lacks, grouped by app. One click brings the ticked ones over;
 * anything replaced is backed up first. Loads its own list, so any screen
 * about this machine can show it.
 */
export function BringOver({ onError }: { onError: (e: string) => void }) {
  const [view, setView] = useState<BringView | null>(null);
  const [skipped, setSkipped] = useState<Set<string>>(new Set());
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<BringResult | null>(null);

  const load = useCallback(() => {
    filesToBring().then(setView, (e) => onError(String(e)));
  }, [onError]);
  useEffect(load, [load]);

  const files = useMemo(() => view?.files.filter((f) => !f.too_large) ?? [], [view]);
  // Sync-app conflict copies start unticked; a tick flips a file from its default.
  const isSkipped = (f: FileToBring) => skipped.has(fileKey(f)) !== isConflictCopy(f.rel);
  const chosen = files.filter((f) => !isSkipped(f));
  const groups = useMemo(() => groupByApp(view?.files ?? []), [view]);
  // One app opens by default; its kinds (Scripts, FX chains, ...) stay folded.
  const folders = useFolders("bring-over", (id) => !id.includes("/") && groups.length === 1);

  if (!view || (view.files.length === 0 && !result)) return null;

  async function bring() {
    setBusy(true);
    try {
      setResult(await bringFiles(chosen.map(fileKey)));
      setSkipped(new Set());
      load();
    } catch (e) {
      onError(String(e));
    } finally {
      setBusy(false);
    }
  }

  const toggle = (k: string) =>
    setSkipped((s) => {
      const n = new Set(s);
      if (n.has(k)) n.delete(k);
      else n.add(k);
      return n;
    });

  return (
    <section className="rounded-lg border border-line bg-panel px-3 py-2.5">
      <header className="flex items-center gap-3">
        <div className="min-w-0 flex-1">
          <h2 className="text-xs font-semibold">Bring over from the Golden Rig</h2>
          <p className="truncate text-[11px] text-dim">
            {view.files.length === 0
              ? "Everything is here."
              : groups.map(([app, fs]) => `${app}: ${summary(fs)}`).join(" · ")}
          </p>
        </div>
        {view.files.length > 0 && (
          <Button variant="quiet" onClick={() => setOpen((o) => !o)} className="shrink-0">
            {open ? "Hide files" : "Show files"}
          </Button>
        )}
        {view.linked && chosen.length > 0 && (
          <Button onClick={bring} disabled={busy} className="shrink-0 whitespace-nowrap">
            {busy ? "Bringing over…" : `Bring over ${chosen.length}`}
          </Button>
        )}
      </header>

      {!view.linked && view.files.length > 0 && (
        <p className="mt-1.5 text-[11px] text-dim">
          Link this machine with the Golden one (Link a machine, top bar) to bring these over.
        </p>
      )}

      {result && (
        <p className="mt-1.5 text-[11px] text-ok">
          Brought over {result.written} file{result.written === 1 ? "" : "s"}.
          {result.backed_up > 0 &&
            ` ${result.backed_up} replaced file${result.backed_up === 1 ? " was" : "s were"} saved first.`}
          {result.written > 0 && " You can undo this in History."}
          {result.unavailable.length > 0 && (
            <span className="text-warn">
              {" "}
              {result.unavailable.length} not available yet: open Patch Pilot on the Golden machine so it can send
              them.
            </span>
          )}
        </p>
      )}

      {open && (
        <div className="mt-2 flex flex-col gap-2">
          <div className="-mx-1">
            {groups.map(([app, fs]) => (
              <Folder
                key={app}
                title={app}
                count={fs.length}
                className="px-1 text-xs"
                open={folders.isOpen(app)}
                onToggle={() => folders.toggle(app)}
              >
                {groupByKind(fs).map(([kind, ks]) => {
                  const id = `${app}/${kind}`;
                  const ticked = ks.filter((f) => !f.too_large && !isSkipped(f)).length;
                  return (
                    <Folder
                      key={id}
                      title={plural(kind)}
                      count={ks.length}
                      className="pl-5 pr-1 text-xs"
                      open={folders.isOpen(id)}
                      onToggle={() => folders.toggle(id)}
                      badges={view.linked && ticked < ks.length && <Badge>{ticked} ticked</Badge>}
                    >
                      <FolderTree
                        node={buildTree(ks, (f) => f.rel.split("/").slice(1, -1), id)}
                        folders={folders}
                        indent={36}
                        renderItems={(items, indent) => (
                          <ul className="flex flex-col" style={{ paddingLeft: indent }}>
                            {items.map((f) => (
                              <FileLine
                                key={fileKey(f)}
                                f={f}
                                linked={view.linked}
                                checked={!isSkipped(f)}
                                onToggle={() => toggle(fileKey(f))}
                              />
                            ))}
                          </ul>
                        )}
                      />
                    </Folder>
                  );
                })}
              </Folder>
            ))}
          </div>
          <p className="text-[11px] text-dim">
            Plug-ins aren't copied: they need their installers and licences. Anything replaced is backed up in Patch
            Pilot's app folder first.
          </p>
        </div>
      )}
    </section>
  );
}

function FileLine({
  f,
  linked,
  checked,
  onToggle,
}: {
  f: FileToBring;
  linked: boolean;
  checked: boolean;
  onToggle: () => void;
}) {
  return (
    <li>
      <label className="flex items-center gap-2 rounded-[5px] px-1 py-1 hover:bg-sel/50">
        <input
          type="checkbox"
          className="accent-accent"
          disabled={f.too_large || !linked}
          checked={!f.too_large && checked}
          onChange={onToggle}
        />
        <span className="min-w-0 flex-1 truncate text-xs" title={f.rel}>
          {fileName(f.rel)}
        </span>
        {isConflictCopy(f.rel) && (
          <span title="A copy a sync app (Dropbox, OneDrive...) made when two machines changed this file at once. Not ticked, so it stays behind.">
            <Badge tone="amber">conflict copy</Badge>
          </span>
        )}
        {f.too_large ? (
          <Badge>Too large to send</Badge>
        ) : (
          f.replaces && <Badge tone="amber">Updates this machine's copy</Badge>
        )}
      </label>
    </li>
  );
}

function groupByKind(files: FileToBring[]): [string, FileToBring[]][] {
  const m = new Map<string, FileToBring[]>();
  for (const f of files) m.set(f.kind, [...(m.get(f.kind) ?? []), f]);
  return [...m.entries()];
}

function groupByApp(files: FileToBring[]): [string, FileToBring[]][] {
  const m = new Map<string, FileToBring[]>();
  for (const f of files) m.set(f.app, [...(m.get(f.app) ?? []), f]);
  return [...m.entries()];
}

/** "3 FX chains, 12 scripts" */
function summary(files: FileToBring[]): string {
  const counts = new Map<string, number>();
  for (const f of files) counts.set(f.kind, (counts.get(f.kind) ?? 0) + 1);
  return [...counts.entries()].map(([k, n]) => `${n} ${n === 1 ? lower(k) : plural(lower(k))}`).join(", ");
}

/** "Track template" → "track template", but "FX chain" stays. */
function lower(s: string): string {
  return s.length > 1 && s[1] === s[1].toLowerCase() ? s[0].toLowerCase() + s.slice(1) : s;
}

function plural(s: string): string {
  return s.endsWith("s") ? s : `${s}s`;
}

function fileName(rel: string): string {
  return rel.slice(rel.lastIndexOf("/") + 1);
}
