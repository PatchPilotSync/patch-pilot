import { useCallback, useEffect, useMemo, useState } from "react";
import {
  onVaultChanged,
  vaultAdd,
  vaultDismiss,
  vaultInspect,
  vaultReveal,
  vaultRun,
  vaultSetRoot,
  vaultStatus,
  type InboxItem,
  type NewEntry,
  type Os,
  type Product,
  type VaultEntry,
  type VaultView,
} from "../api";
import { Badge, Button, inputClass, timeAgo } from "../ui";

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
  const [view, setView] = useState<VaultView | null>(null);
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
  }, [onError, onCount]);

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

  if (!view) return null;

  if (!view.root || changing) {
    return <PickFolder view={view} onChoose={choose} onCancel={view.root ? () => setChanging(false) : null} />;
  }

  const waiting = [...extra, ...view.inbox.filter((i) => !extra.some((x) => x.path === i.path))];

  async function add(item: InboxItem, meta: NewEntry) {
    try {
      const r = await vaultAdd(item.path, meta);
      const lines = [r.already ? `${item.file_name} is already in the vault.` : `Added ${item.file_name} to the vault.`];
      if (r.signer_warning) lines.push(r.signer_warning);
      onNotice(lines);
      setExtra((x) => x.filter((i) => i.path !== item.path));
      refresh();
    } catch (e) {
      onError(String(e));
    }
  }

  async function skip(item: InboxItem) {
    setExtra((x) => x.filter((i) => i.path !== item.path));
    if (!view?.inbox.some((i) => i.path === item.path)) return;
    try {
      await vaultDismiss(item.path);
      refresh();
    } catch (e) {
      onError(String(e));
    }
  }

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
      {view.problem && <p className="px-4 pt-3 text-xs text-bad">{view.problem}</p>}

      {waiting.length > 0 && (
        <section>
          <h3 className="px-4 pb-0.5 pt-4 text-[11px] text-dim">
            New installers{view.downloads ? " in Downloads" : ""}: add them to keep a copy
          </h3>
          <ul>
            {waiting.map((item) => (
              <InboxRow
                key={item.path}
                item={item}
                products={view.products}
                onAdd={(meta) => add(item, meta)}
                onSkip={() => skip(item)}
              />
            ))}
          </ul>
        </section>
      )}

      <Entries entries={view.entries} onError={onError} />
      {view.entries.length === 0 && waiting.length === 0 && (
        <p className="px-4 pt-4 text-xs text-dim">
          Nothing here yet. Plug-in installers you download show up here to add; you can also drop one on this window.
        </p>
      )}
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
    <div className="mx-auto flex max-w-lg flex-col gap-3 px-5 py-8">
      <h1 className="text-sm font-semibold">Installer Vault</h1>
      <p className="text-xs text-dim">
        Keeps a copy of the plug-in installers you download, sorted by vendor and version, so a plug-in missing on
        another machine is a click away. Choose any folder: this one, an external drive, a NAS or a cloud folder you
        already use. Patch Pilot never uploads installers, and your Downloads stay as they are.
      </p>
      <label className="flex flex-col gap-1 text-xs text-dim">
        Vault folder (or drop a folder on this window)
        <input className={inputClass} value={path} onChange={(e) => setPath(e.target.value)} spellCheck={false} />
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

function InboxRow({
  item,
  products,
  onAdd,
  onSkip,
}: {
  item: InboxItem;
  products: Product[];
  onAdd: (meta: NewEntry) => void;
  onSkip: () => void;
}) {
  const [editing, setEditing] = useState(false);
  const [busy, setBusy] = useState(false);
  const [productId, setProductId] = useState(item.product?.canonical_id ?? "");
  const [vendor, setVendor] = useState(item.product?.vendor ?? "");
  const [name, setName] = useState(item.product?.name ?? item.guess.name);
  const [version, setVersion] = useState(item.guess.version ?? "");
  const [os, setOs] = useState<Os | "">(item.guess.os ?? "");

  function pick(id: string) {
    setProductId(id);
    const p = products.find((p) => p.canonical_id === id);
    if (p) {
      setVendor(p.vendor);
      setName(p.name);
    }
  }

  async function add() {
    setBusy(true);
    await onAdd({
      vendor: vendor.trim(),
      product: name.trim() || item.guess.name,
      product_id: productId || null,
      version: version.trim() || null,
      os: os || null,
      arch: item.guess.arch,
    });
    setBusy(false);
  }

  const summary = [vendor && name ? `${vendor} ${name}` : name, version, os ? OS_LABEL[os] : "any OS"]
    .filter(Boolean)
    .join(" · ");

  return (
    <li className="px-4 py-1.5 hover:bg-sel/40">
      <div className="flex items-center gap-2">
        <span className="min-w-0 flex-1">
          <span className="block truncate" title={item.path}>
            {item.file_name}
          </span>
          <span className="block truncate text-xs text-dim">
            {productId ? summary : `${summary} · not matched to a plug-in you have`} · {formatSize(item.size)} ·{" "}
            {timeAgo(item.modified)}
          </span>
        </span>
        <Button variant="quiet" onClick={() => setEditing(!editing)}>
          {editing ? "Done" : "Edit"}
        </Button>
        <Button variant="quiet" onClick={onSkip} title="Don't offer this file again">
          Skip
        </Button>
        <Button onClick={add} disabled={busy}>
          {busy ? "Adding…" : "Add"}
        </Button>
      </div>
      {editing && (
        <div className="mt-1.5 grid grid-cols-[1fr_1fr] gap-1.5 text-xs">
          <select className={`${inputClass} col-span-2`} value={productId} onChange={(e) => pick(e.target.value)}>
            <option value="">Not one of my plug-ins</option>
            {products.map((p) => (
              <option key={p.canonical_id} value={p.canonical_id}>
                {p.vendor ? `${p.vendor} · ${p.name}` : p.name}
              </option>
            ))}
          </select>
          <input className={inputClass} placeholder="Vendor" value={vendor} onChange={(e) => setVendor(e.target.value)} />
          <input className={inputClass} placeholder="Product" value={name} onChange={(e) => setName(e.target.value)} />
          <input className={inputClass} placeholder="Version" value={version} onChange={(e) => setVersion(e.target.value)} />
          <select className={inputClass} value={os} onChange={(e) => setOs(e.target.value as Os | "")}>
            <option value="windows">Windows</option>
            <option value="macos">macOS</option>
            <option value="">Either (e.g. a zip with both)</option>
          </select>
        </div>
      )}
    </li>
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
      {groups.map(([vendor, rows]) => (
        <section key={vendor}>
          <h3 className="px-4 pb-0.5 pt-4 text-[11px] text-dim">{vendor}</h3>
          <ul>
            {rows.map((e) => (
              <li key={e.id} className="group flex h-7 items-center gap-2 px-4 hover:bg-sel/60">
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
        </section>
      ))}
    </>
  );
}

function formatSize(bytes: number): string {
  if (bytes >= 1e9) return `${(bytes / 1e9).toFixed(1)} GB`;
  if (bytes >= 1e6) return `${Math.round(bytes / 1e6)} MB`;
  return `${Math.max(1, Math.round(bytes / 1e3))} KB`;
}
