import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  vaultAdd,
  vaultRemove,
  vaultSkip,
  type InboxItem,
  type NewEntry,
  type Os,
  type Product,
} from "../api";
import { Badge, Button, Folder, FolderControls, inputClass, timeAgo, useFolders } from "../ui";

const OS_LABEL: Record<Os, string> = { windows: "Windows", macos: "macOS" };
const UNMATCHED = "Not matched to your plug-ins";

/** What happened to a row this session. Rows stay in place (and undoable) until the tab closes. */
type Outcome =
  | { kind: "copying" }
  | { kind: "added"; entryId: string; where: string }
  | { kind: "already" }
  | { kind: "skipped" };

/** The guess, or the user's correction of it. */
function metaOf(item: InboxItem, edits: Record<string, NewEntry>): NewEntry {
  return (
    edits[item.path] ?? {
      vendor: item.product?.vendor ?? "",
      product: item.product?.name ?? item.guess.name,
      product_id: item.product?.canonical_id ?? null,
      version: item.guess.version,
      os: item.guess.os,
      arch: item.guess.arch,
    }
  );
}

/**
 * Review queue for installers waiting in Downloads: grouped by vendor, one
 * key per decision (A add, S skip, E edit, arrows to move, Z undo), bulk add
 * per vendor, and every add or skip undoable in place.
 */
export function Triage({
  items,
  products,
  fromDownloads,
  onChanged,
  onError,
  onNotice,
}: {
  items: InboxItem[];
  products: Product[];
  fromDownloads: boolean;
  onChanged: () => void;
  onError: (e: string) => void;
  onNotice: (lines: string[]) => void;
}) {
  // Rows keep their place after an add or skip, so the list doesn't jump
  // under the cursor; new downloads join at the end.
  const [shown, setShown] = useState<InboxItem[]>(items);
  const [done, setDone] = useState<Record<string, Outcome>>({});
  const [edits, setEdits] = useState<Record<string, NewEntry>>({});
  const [focus, setFocus] = useState<string | null>(null);
  const [editing, setEditing] = useState<string | null>(null);
  const [history, setHistory] = useState<string[]>([]);
  const [toast, setToast] = useState<{ text: string; at: number } | null>(null);
  const queue = useRef<Promise<unknown>>(Promise.resolve());
  const rowRefs = useRef<Record<string, HTMLLIElement | null>>({});
  const folders = useFolders("vault-inbox", (id) => id !== UNMATCHED);

  useEffect(() => {
    setShown((prev) => {
      const live = new Set(items.map((i) => i.path));
      const kept = prev.filter((i) => live.has(i.path) || done[i.path]);
      const known = new Set(kept.map((i) => i.path));
      return [...kept.map((i) => items.find((n) => n.path === i.path) ?? i), ...items.filter((i) => !known.has(i.path))];
    });
    // `done` is read, not tracked: a finished row stays until the tab closes.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [items]);

  const groups = useMemo(() => {
    const by = new Map<string, InboxItem[]>();
    for (const i of shown) {
      const m = metaOf(i, edits);
      const key = m.product_id ? m.vendor || "Unknown vendor" : UNMATCHED;
      by.set(key, [...(by.get(key) ?? []), i]);
    }
    return [...by.entries()].sort(([a], [b]) =>
      a === UNMATCHED ? 1 : b === UNMATCHED ? -1 : a.localeCompare(b, undefined, { sensitivity: "base" }),
    );
  }, [shown, edits]);

  const pending = (i: InboxItem) => !done[i.path];
  // Keyboard order: the pending rows in open folders, top to bottom.
  const order = useMemo(
    () => groups.filter(([g]) => folders.isOpen(g)).flatMap(([, rows]) => rows.filter(pending).map((i) => i.path)),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [groups, done, folders.isOpen],
  );
  const left = shown.filter(pending).length;
  const matchedLeft = shown.filter((i) => pending(i) && metaOf(i, edits).product_id);

  useEffect(() => {
    if (focus && order.includes(focus)) return;
    setFocus(order[0] ?? null);
  }, [order, focus]);

  useEffect(() => {
    if (focus) rowRefs.current[focus]?.scrollIntoView({ block: "nearest" });
  }, [focus]);

  useEffect(() => {
    if (!toast) return;
    const t = setTimeout(() => setToast(null), 6000);
    return () => clearTimeout(t);
  }, [toast]);

  const advance = useCallback(
    (from: string) => {
      const i = order.indexOf(from);
      const rest = order.filter((p) => p !== from);
      setFocus(rest[Math.min(Math.max(i, 0), rest.length - 1)] ?? null);
    },
    [order],
  );

  function add(items: InboxItem[], corrected?: NewEntry) {
    if (items.length === 0) return;
    setEditing(null);
    setDone((d) => ({ ...d, ...Object.fromEntries(items.map((i) => [i.path, { kind: "copying" } as Outcome])) }));
    if (items.length === 1) advance(items[0].path);
    for (const item of items) {
      const meta = corrected ?? metaOf(item, edits);
      // One copy at a time, in order: big installers don't fight for the disk.
      queue.current = queue.current.then(async () => {
        try {
          const r = await vaultAdd(item.path, meta);
          const where = [meta.vendor, meta.product, meta.version].filter(Boolean).join(" / ");
          setDone((d) => ({
            ...d,
            [item.path]: r.already ? { kind: "already" } : { kind: "added", entryId: r.entry.id, where },
          }));
          if (!r.already) setHistory((h) => [...h, item.path]);
          setToast({ text: r.already ? `${meta.product} was already in the vault` : `Added ${meta.product}`, at: Date.now() });
          if (r.signer_warning) onNotice([r.signer_warning]);
        } catch (e) {
          setDone((d) => {
            const { [item.path]: _, ...rest } = d;
            return rest;
          });
          onError(String(e));
        }
      });
    }
    queue.current = queue.current.then(onChanged);
  }

  async function skip(items: InboxItem[]) {
    if (items.length === 0) return;
    setEditing(null);
    setDone((d) => ({ ...d, ...Object.fromEntries(items.map((i) => [i.path, { kind: "skipped" } as Outcome])) }));
    setHistory((h) => [...h, ...items.map((i) => i.path)]);
    if (items.length === 1) advance(items[0].path);
    setToast({ text: items.length === 1 ? `Skipped ${items[0].file_name}` : `Skipped ${items.length} files`, at: Date.now() });
    try {
      await vaultSkip(items.map((i) => i.path));
      onChanged();
    } catch (e) {
      onError(String(e));
    }
  }

  async function undo(path: string) {
    const o = done[path];
    if (!o || o.kind === "copying") return;
    try {
      if (o.kind === "added") await vaultRemove(o.entryId);
      if (o.kind === "skipped") await vaultSkip([path], false);
      setDone((d) => {
        const { [path]: _, ...rest } = d;
        return rest;
      });
      setHistory((h) => h.filter((p) => p !== path));
      setFocus(path);
      setToast(null);
      onChanged();
    } catch (e) {
      onError(String(e));
    }
  }

  // Keys work anywhere on the tab, except while typing.
  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      const t = e.target as HTMLElement;
      if (t.closest("input, select, textarea")) return;
      const key = e.key.toLowerCase();
      // A focused button handles its own Enter and Space.
      if (t.closest("button") && (key === "enter" || key === " ")) return;
      if (key === "z" && !e.altKey) {
        const last = history[history.length - 1];
        if (last) {
          e.preventDefault();
          undo(last);
        }
        return;
      }
      if (e.metaKey || e.ctrlKey || e.altKey) return;
      const item = shown.find((i) => i.path === focus);
      const i = focus ? order.indexOf(focus) : -1;
      if (key === "arrowdown" || key === "j") setFocus(order[Math.min(i + 1, order.length - 1)] ?? null);
      else if (key === "arrowup" || key === "k") setFocus(order[Math.max(i - 1, 0)] ?? null);
      else if (item && (key === "a" || key === "enter")) add([item]);
      else if (item && key === "s") skip([item]);
      else if (item && key === "e") setEditing(item.path);
      else return;
      e.preventDefault();
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  if (shown.length === 0) return null;

  return (
    <section className="border-b border-line pb-2">
      {shown.length > 0 && (
        <div className="flex items-center gap-3 px-4 pb-1 pt-3">
          <div className="min-w-0 flex-1">
            <h2 className="text-xs font-semibold">
              {left === 0
                ? "All caught up"
                : `${left} installer${left === 1 ? "" : "s"}${fromDownloads ? " in your Downloads" : ""} to review`}
            </h2>
            <p className="truncate text-[11px] text-dim">
              <Kbd>A</Kbd> add · <Kbd>S</Kbd> skip · <Kbd>E</Kbd> fix the name · <Kbd>↑</Kbd>
              <Kbd>↓</Kbd> move · <Kbd>Z</Kbd> undo. Your Downloads stay as they are.
            </p>
          </div>
          {matchedLeft.length > 1 && (
            <Button onClick={() => add(matchedLeft)}>Add all {matchedLeft.length} matched</Button>
          )}
        </div>
      )}
      <FolderControls ids={groups.map(([g]) => g)} folders={folders} className="px-3" />
      {groups.map(([group, rows]) => {
        const open = rows.filter(pending);
        const added = rows.filter((r) => done[r.path]?.kind === "added").length;
        return (
          <Folder
            key={group}
            title={group}
            count={rows.length}
            open={folders.isOpen(group)}
            onToggle={() => folders.toggle(group)}
            badges={
              open.length === 0 ? (
                <span className="text-[11px] text-ok">✓ Done</span>
              ) : (
                <span className="text-[11px] text-dim">
                  {open.length} to review{added > 0 ? ` · ${added} added` : ""}
                </span>
              )
            }
          >
            {open.length > 1 && (
              <div className="flex gap-1 pb-0.5 pl-7 pr-4">
                {group !== UNMATCHED && (
                  <Button variant="quiet" className="h-6 px-2" onClick={() => add(open)}>
                    Add all {open.length}
                  </Button>
                )}
                <Button variant="quiet" className="h-6 px-2" onClick={() => skip(open)}>
                  Skip all {open.length}
                </Button>
              </div>
            )}
            <ul className="pb-1">
              {rows.map((item) => (
                <Row
                  key={item.path}
                  ref={(el) => {
                    rowRefs.current[item.path] = el;
                  }}
                  item={item}
                  meta={metaOf(item, edits)}
                  outcome={done[item.path]}
                  focused={focus === item.path}
                  editing={editing === item.path}
                  products={products}
                  onFocus={() => setFocus(item.path)}
                  onAdd={() => add([item])}
                  onSkip={() => skip([item])}
                  onUndo={() => undo(item.path)}
                  onEdit={() => setEditing(editing === item.path ? null : item.path)}
                  onSave={(m, andAdd) => {
                    setEdits((x) => ({ ...x, [item.path]: m }));
                    setEditing(null);
                    if (andAdd) add([item], m);
                  }}
                />
              ))}
            </ul>
          </Folder>
        );
      })}
      {toast && (
        <div className="pointer-events-none fixed inset-x-0 bottom-4 z-20 flex justify-center">
          <div className="pointer-events-auto flex items-center gap-3 rounded-md border border-line bg-panel px-3 py-1.5 text-xs shadow-lg">
            <span className="text-ok">✓</span>
            <span>{toast.text}</span>
            {history.length > 0 && (
              <button className="text-accent-text hover:underline" onClick={() => undo(history[history.length - 1])}>
                Undo
              </button>
            )}
          </div>
        </div>
      )}
    </section>
  );
}

/** Installers the user skipped, listed under the vault so they can be brought back. */
export function Skipped({
  items,
  onChanged,
  onError,
}: {
  items: InboxItem[];
  onChanged: () => void;
  onError: (e: string) => void;
}) {
  const folders = useFolders("vault-skipped");
  if (items.length === 0) return null;
  return (
    <div className="mt-2 border-t border-line pt-1">
      <Folder
        title="Skipped installers"
        count={items.length}
        open={folders.isOpen("skipped")}
        onToggle={() => folders.toggle("skipped")}
      >
        <ul className="pb-1">
          {items.map((i) => (
            <li key={i.path} className="group flex h-7 items-center gap-2 pl-8 pr-4 hover:bg-sel/60">
              <span className="min-w-0 flex-1 truncate text-dim" title={i.path}>
                {i.file_name}
              </span>
              <Button
                variant="quiet"
                className="h-6 px-2 opacity-0 group-hover:opacity-100 focus:opacity-100"
                onClick={() => vaultSkip([i.path], false).then(onChanged, (e) => onError(String(e)))}
              >
                Bring back
              </Button>
            </li>
          ))}
        </ul>
      </Folder>
    </div>
  );
}

function Kbd({ children }: { children: string }) {
  return (
    <kbd className="mx-px inline-block min-w-[1.25em] rounded border border-line bg-panel px-1 text-center font-sans text-[10px] leading-[1.4]">
      {children}
    </kbd>
  );
}

function Row({
  ref,
  item,
  meta,
  outcome,
  focused,
  editing,
  products,
  onFocus,
  onAdd,
  onSkip,
  onUndo,
  onEdit,
  onSave,
}: {
  ref: (el: HTMLLIElement | null) => void;
  item: InboxItem;
  meta: NewEntry;
  outcome: Outcome | undefined;
  focused: boolean;
  editing: boolean;
  products: Product[];
  onFocus: () => void;
  onAdd: () => void;
  onSkip: () => void;
  onUndo: () => void;
  onEdit: () => void;
  onSave: (m: NewEntry, andAdd: boolean) => void;
}) {
  if (outcome) {
    const text = {
      copying: "Copying to the vault…",
      added: outcome.kind === "added" ? `Added to ${outcome.where}` : "",
      already: "Already in the vault",
      skipped: "Skipped",
    }[outcome.kind];
    return (
      <li ref={ref} className="flex h-7 items-center gap-2 pl-8 pr-4 text-xs text-dim">
        <span className={`w-3 shrink-0 text-center ${outcome.kind === "skipped" ? "" : "text-ok"}`}>
          {outcome.kind === "copying" ? <Spinner /> : outcome.kind === "skipped" ? "–" : "✓"}
        </span>
        <span className="min-w-0 flex-1 truncate">
          <span className="text-fg/70">{meta.product}</span> · {text}
        </span>
        {(outcome.kind === "added" || outcome.kind === "skipped") && (
          <button className="text-accent-text hover:underline" onClick={onUndo}>
            Undo
          </button>
        )}
      </li>
    );
  }
  return (
    <li
      ref={ref}
      onMouseDown={onFocus}
      className={`relative pl-8 pr-4 ${focused ? "bg-sel" : "hover:bg-sel/50"}`}
    >
      {focused && <span className="absolute inset-y-1 left-3 w-[3px] rounded-full bg-accent" aria-hidden="true" />}
      <div className="flex min-h-11 items-center gap-2 py-1">
        <span className="min-w-0 flex-1">
          <span className="flex items-baseline gap-2">
            <span className="truncate font-medium">{meta.product || item.file_name}</span>
            {meta.version && <span className="font-mono text-[11px] text-dim">{meta.version}</span>}
            <span className="text-[11px] text-dim">{meta.os ? OS_LABEL[meta.os] : "Any OS"}</span>
            {!meta.product_id && <Badge tone="amber">check the name</Badge>}
          </span>
          <span className="block truncate text-[11px] text-dim" title={item.path}>
            {item.file_name} · {formatSize(item.size)} · {timeAgo(item.modified)}
          </span>
        </span>
        <span className="flex gap-1">
          <Button variant="quiet" className="px-2" onClick={onEdit} title="Fix the name, version or OS (E)">
            {editing ? "Cancel" : "Edit"}
          </Button>
          <Button variant="quiet" className="px-2" onClick={onSkip} title="Not for the vault; won't be offered again (S)">
            Skip
          </Button>
          <Button
            variant={focused ? "primary" : "link"}
            className={focused ? "" : "px-2"}
            onClick={onAdd}
            title="Copy into the vault (A)"
          >
            Add
          </Button>
        </span>
      </div>
      {editing && <Editor meta={meta} products={products} onSave={onSave} onCancel={onEdit} />}
    </li>
  );
}

/** Correct the guess: pick one of your plug-ins (type to search), version and OS. Enter adds. */
function Editor({
  meta,
  products,
  onSave,
  onCancel,
}: {
  meta: NewEntry;
  products: Product[];
  onSave: (m: NewEntry, andAdd: boolean) => void;
  onCancel: () => void;
}) {
  const label = (p: Product) => (p.vendor ? `${p.vendor} · ${p.name}` : p.name);
  const current = products.find((p) => p.canonical_id === meta.product_id);
  const [query, setQuery] = useState(current ? label(current) : [meta.vendor, meta.product].filter(Boolean).join(" "));
  const [version, setVersion] = useState(meta.version ?? "");
  const [os, setOs] = useState<Os | "">(meta.os ?? "");
  const first = useRef<HTMLInputElement>(null);
  useEffect(() => {
    first.current?.focus();
    first.current?.select();
  }, []);

  function result(): NewEntry {
    const p = products.find((p) => label(p) === query.trim());
    return {
      vendor: p ? p.vendor : meta.product_id ? "" : meta.vendor,
      product: p ? p.name : query.trim() || meta.product,
      product_id: p?.canonical_id ?? null,
      version: version.trim() || null,
      os: os || null,
      arch: meta.arch,
    };
  }

  function onKey(e: React.KeyboardEvent) {
    if (e.key === "Enter") {
      e.preventDefault();
      onSave(result(), true);
    } else if (e.key === "Escape") {
      e.preventDefault();
      onCancel();
    }
  }

  return (
    <div className="grid grid-cols-[1fr_6rem_auto] items-center gap-1.5 pb-2 text-xs" onKeyDown={onKey}>
      <input
        ref={first}
        list="vault-products"
        className={inputClass}
        placeholder="Plug-in (type to search yours)"
        value={query}
        onChange={(e) => setQuery(e.target.value)}
      />
      <datalist id="vault-products">
        {products.map((p) => (
          <option key={p.canonical_id} value={label(p)} />
        ))}
      </datalist>
      <input className={inputClass} placeholder="Version" value={version} onChange={(e) => setVersion(e.target.value)} />
      <span className="flex rounded-md border border-line p-px">
        {(["windows", "macos", ""] as const).map((o) => (
          <button
            key={o}
            onClick={() => setOs(o)}
            className={`h-6 rounded-[5px] px-2 ${os === o ? "bg-sel font-medium text-fg" : "text-dim hover:text-fg"}`}
          >
            {o ? OS_LABEL[o] : "Either"}
          </button>
        ))}
      </span>
      <span className="col-span-3 flex items-center gap-2">
        <Button onClick={() => onSave(result(), true)}>Add</Button>
        <Button variant="quiet" onClick={() => onSave(result(), false)}>
          Save
        </Button>
        <span className="text-[11px] text-dim">Enter adds · Esc cancels</span>
      </span>
    </div>
  );
}

function Spinner() {
  return (
    <svg width="10" height="10" viewBox="0 0 10 10" className="inline animate-spin" aria-hidden="true">
      <path d="M5 1a4 4 0 1 1-4 4" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" />
    </svg>
  );
}

export function formatSize(bytes: number): string {
  if (bytes >= 1e9) return `${(bytes / 1e9).toFixed(1)} GB`;
  if (bytes >= 1e6) return `${Math.round(bytes / 1e6)} MB`;
  return `${Math.max(1, Math.round(bytes / 1e3))} KB`;
}
