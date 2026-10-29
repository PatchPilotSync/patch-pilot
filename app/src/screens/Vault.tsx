import { useCallback, useEffect, useMemo, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  onVaultChanged,
  vaultInspect,
  vaultReveal,
  vaultRun,
  vaultSetRoot,
  vaultStatus,
  type InboxItem,
  type Os,
  type VaultEntry,
  type VaultView,
} from "../api";
import { Badge, Button, Folder, FolderControls, Label, Loading, Notice, Panel, Tile, inputClass, useFolders } from "../ui";
import { Skipped, Triage } from "./VaultTriage";

/** The last Vault shown, so reopening the tab shows it at once while it refreshes. */
let lastView: VaultView | null = null;

const OS_LABEL: Record<Os, string> = { windows: "Windows", macos: "macOS" };

/** Installer Vault: pick the folder, add installers from Downloads, run them later. */
export function Vault({
  dropped,
  onError,
  onNotice,
  onCount,
}: {
  /** Files or folders dropped on the window while this tab is open. */
  dropped: { paths: string[]; at: number } | null;
  onError: (e: string) => void;
  onNotice: (lines: string[]) => void;
  /** How many new installers are waiting, for the tab's badge. */
  onCount: (n: number) => void;
}) {
  const [view, setViewState] = useState<VaultView | null>(lastView);
  const setView = useCallback((v: VaultView) => {
    lastView = v;
    setViewState(v);
  }, []);
  const [extra, setExtra] = useState<InboxItem[]>([]);
  const [changing, setChanging] = useState(false);

  const refresh = useCallback(() => {
    vaultStatus().then(
      (v) => {
        setView(v);
        onCount(v.inbox.length);
      },
      (e) => onError(String(e)),
    );
  }, [onError, onCount, setView]);

  useEffect(() => {
    refresh();
    const un = onVaultChanged(refresh);
    return () => {
      un.then((f) => f());
    };
  }, [refresh]);

  // Dropped installers join the list; a dropped folder becomes the vault folder.
  useEffect(() => {
    if (!dropped || !view) return;
    vaultInspect(dropped.paths).then((items) => {
      if (items.length > 0) {
        setExtra((x) => [...items.filter((i) => !x.some((o) => o.path === i.path)), ...x]);
      } else if (dropped.paths.length === 1 && (!view.root || changing)) {
        choose(dropped.paths[0]);
      }
    }, (e) => onError(String(e)));
  }, [dropped]);

  async function choose(path: string) {
    try {
      setView(await vaultSetRoot(path));
      setChanging(false);
    } catch (e) {
      onError(String(e));
    }
  }

  if (!view) return <Loading text="Opening the Installer Vault…" compact />;

  if (!view.root || changing) {
    return <PickFolder view={view} onChoose={choose} onCancel={view.root ? () => setChanging(false) : null} />;
  }

  const waiting = [...extra, ...view.inbox.filter((i) => !extra.some((x) => x.path === i.path))];

  return (
    <div className="flex flex-col pb-6">
      <header className="flex items-center gap-3 border-b border-line px-4 py-2.5">
        <div className="min-w-0 flex-1">
          <h1 className="text-sm font-semibold">Installer Vault</h1>
          <p className="truncate text-xs text-dim" title={view.root}>
            Your installers, kept in {view.root}. Up To Speed offers them when a machine is missing a plug-in.
          </p>
        </div>
        <Button variant="quiet" onClick={() => vaultReveal().catch((e) => onError(String(e)))}>
          Open folder
        </Button>
        <Button variant="quiet" onClick={() => setChanging(true)}>
          Change folder
        </Button>
      </header>
      {view.problem && (
        <Panel className="mx-3 mt-3 p-1">
          <Notice tone="bad" icon="info" text={<span className="text-bad">{view.problem}</span>} />
        </Panel>
      )}

      <Triage
        items={waiting}
        products={view.products}
        fromDownloads={!!view.downloads}
        onChanged={refresh}
        onError={onError}
        onNotice={onNotice}
      />

      <Entries entries={view.entries} onError={onError} />
      {view.entries.length === 0 && waiting.length === 0 && (
        <Panel className="mx-3 mt-3 p-1">
          <Notice
            tone="dim"
            icon="box"
            text={
              <span className="text-dim">
                Nothing here yet. Plug-in installers you download show up here to add; you can also drop one on this
                window.
              </span>
            }
          />
        </Panel>
      )}
      <Skipped items={view.skipped} onChanged={refresh} onError={onError} />
    </div>
  );
}

function PickFolder({
  view,
  onChoose,
  onCancel,
}: {
  view: VaultView;
  onChoose: (path: string) => void;
  onCancel: (() => void) | null;
}) {
  const [path, setPath] = useState(view.root ?? view.suggested_root ?? "");
  return (
    <div className="mx-auto flex max-w-lg flex-col gap-4 px-6 py-8">
      <div className="flex items-start gap-3">
        <Tile name="box" className="mt-px" />
        <div>
          <h1 className="text-base font-semibold">Installer Vault</h1>
          <p className="mt-1 text-xs leading-relaxed text-dim">
            Keeps a copy of the plug-in installers you download, sorted by vendor and version, so a plug-in missing on
            another machine is a click away. Choose any folder: this one, an external drive, a NAS or a cloud folder you
            already use. Patch Pilot never uploads installers, and your Downloads stay as they are.
          </p>
        </div>
      </div>
      <label className="flex flex-col gap-1.5 rounded-lg border border-line bg-panel px-3 py-2.5 text-xs text-dim">
        Vault folder (or drop a folder on this window)
        <span className="flex gap-1.5">
          <input
            className={`${inputClass} min-w-0 flex-1`}
            value={path}
            onChange={(e) => setPath(e.target.value)}
            spellCheck={false}
          />
          <Button
            variant="quiet"
            onClick={async () => {
              const picked = await open({ directory: true, defaultPath: path || undefined }).catch(() => null);
              if (typeof picked === "string") setPath(picked);
            }}
          >
            Choose…
          </Button>
        </span>
      </label>
      <div className="flex gap-2">
        <Button disabled={!path.trim()} onClick={() => onChoose(path)}>
          Use this folder
        </Button>
        {onCancel && (
          <Button variant="quiet" onClick={onCancel}>
            Cancel
          </Button>
        )}
      </div>
    </div>
  );
}

function Entries({ entries, onError }: { entries: VaultEntry[]; onError: (e: string) => void }) {
  const groups = useMemo(() => {
    const by = new Map<string, VaultEntry[]>();
    for (const e of entries) {
      const k = e.vendor || "Unknown vendor";
      by.set(k, [...(by.get(k) ?? []), e]);
    }
    return [...by.entries()];
  }, [entries]);
  const [running, setRunning] = useState<string | null>(null);
  const folders = useFolders("vault");

  async function run(e: VaultEntry) {
    setRunning(e.id);
    try {
      await vaultRun(e.id);
    } catch (err) {
      onError(String(err));
    } finally {
      setRunning(null);
    }
  }

  return (
    <>
      {entries.length > 0 && (
        <div className="flex items-center gap-3 pb-1 pl-4 pr-3 pt-4">
          <Label className="flex-1">
            In the vault · <span className="tabular-nums">{entries.length}</span> installer
            {entries.length === 1 ? "" : "s"}
          </Label>
          <FolderControls ids={groups.map(([v]) => v)} folders={folders} className="-my-1" />
        </div>
      )}
      {entries.length > 0 && (
        <Panel divided className="mx-3">
          {groups.map(([vendor, rows]) => {
            const missing = rows.filter((e) => e.missing).length;
            return (
              <Folder
                key={vendor}
                title={vendor}
                count={rows.length}
                icon={<Tile name="box" tone={missing > 0 ? "warn" : "accent"} />}
                className="pl-3 pr-4"
                open={folders.isOpen(vendor)}
                onToggle={() => folders.toggle(vendor)}
                badges={missing > 0 && <Badge tone="amber">{missing} file{missing === 1 ? "" : "s"} missing</Badge>}
              >
                <ul className="pb-1">
                  {rows.map((e) => (
                    <li key={e.id} className="group flex h-7 items-center gap-2 pl-16 pr-4 hover:bg-sel/60">
                      <span className="min-w-0 flex-1 truncate" title={e.file}>
                        {e.product}
                        {e.version && <span className="ml-2 font-mono text-[11px] text-dim">{e.version}</span>}
                      </span>
                      {e.missing && <Badge tone="amber">file missing</Badge>}
                      {!e.signer && !e.source_name.toLowerCase().endsWith(".zip") && (
                        <span title="The OS found no signature on this installer">
                          <Badge>unsigned</Badge>
                        </span>
                      )}
                      <span className="w-16 text-right text-[11px] text-dim" title={e.signer ? `Signed by ${e.signer}` : undefined}>
                        {e.os ? OS_LABEL[e.os] : "Any OS"}
                      </span>
                      <span className="flex w-24 justify-end gap-1 opacity-0 group-hover:opacity-100 focus-within:opacity-100">
                        <Button variant="quiet" className="px-2" onClick={() => vaultReveal(e.id).catch((x) => onError(String(x)))}>
                          Show
                        </Button>
                        {e.fits_here && !e.missing && (
                          <Button variant="quiet" className="px-2" disabled={running === e.id} onClick={() => run(e)}>
                            {running === e.id ? "Checking…" : "Run"}
                          </Button>
                        )}
                      </span>
                    </li>
                  ))}
                </ul>
              </Folder>
            );
          })}
        </Panel>
      )}
    </>
  );
}
