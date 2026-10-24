import { useCallback, useEffect, useState, type ReactNode } from "react";
import {
  goBack,
  goBackPlan,
  history as loadHistory,
  undoChange,
  undoPreview,
  type FilePreview,
  type GoBackPlan,
  type HistoryEntry,
  type HistoryView,
  type ProductChange,
} from "../api";
import { Badge, Button } from "../ui";

interface Props {
  /** Bumped when another window or a rescan changed the data. */
  refreshKey: number;
  onError: (e: string) => void;
}

/** One open panel at a time: an undo to confirm or a scan to go back to. */
type Open =
  | { kind: "undo"; id: number; files: FilePreview[] }
  | { kind: "back"; id: number; plan: GoBackPlan }
  | { kind: "details"; id: number };

export function History({ refreshKey, onError }: Props) {
  const [view, setView] = useState<HistoryView | null>(null);
  const [open, setOpen] = useState<Open | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    loadHistory().then(setView, (e) => onError(String(e)));
  }, [refreshKey, onError]);

  const run = useCallback(
    async (f: () => Promise<HistoryView | void>) => {
      setBusy(true);
      try {
        const v = await f();
        if (v) {
          setView(v);
          setOpen(null);
        }
      } catch (e) {
        onError(String(e));
      } finally {
        setBusy(false);
      }
    },
    [onError],
  );

  if (!view) return null;

  return (
    <div className="flex flex-col pb-6">
      <header className="border-b border-line px-4 py-2.5">
        <h1 className="text-sm font-semibold">History</h1>
        <p className="text-xs text-dim">
          Scans that found a change, and every file Patch Pilot changed here. Files go back exactly; plug-ins are
          listed for you to change, never uninstalled.
        </p>
      </header>
      {view.entries.length === 0 && (
        <p className="px-4 py-3 text-dim">Nothing yet. Changes show up here as they happen.</p>
      )}
      <ul>
        {view.entries.map((e) => {
          const key = e.type === "scan" ? `s${e.snapshot_id}` : `f${e.id}`;
          const id = e.type === "scan" ? e.snapshot_id : e.id;
          const isOpen = open !== null && open.id === id && (e.type === "scan") === (open.kind !== "undo");
          return (
            <li key={key} className="border-b border-line">
              {e.type === "scan" ? (
                <ScanRow
                  e={e}
                  busy={busy}
                  open={isOpen ? open : null}
                  onToggle={() => setOpen(isOpen && open?.kind === "details" ? null : { kind: "details", id })}
                  onGoBack={() =>
                    run(async () => setOpen({ kind: "back", id, plan: await goBackPlan(e.snapshot_id) }))
                  }
                  onConfirm={() => run(() => goBack(e.snapshot_id))}
                  onCancel={() => setOpen(null)}
                />
              ) : (
                <FilesRow
                  e={e}
                  busy={busy}
                  open={isOpen && open?.kind === "undo" ? open.files : null}
                  onUndo={() => run(async () => setOpen({ kind: "undo", id, files: await undoPreview(e.id) }))}
                  onConfirm={() => run(() => undoChange(e.id))}
                  onCancel={() => setOpen(null)}
                />
              )}
            </li>
          );
        })}
      </ul>
    </div>
  );
}

function when(at: number): string {
  return new Date(at * 1000).toLocaleString(undefined, {
    day: "numeric",
    month: "short",
    hour: "numeric",
    minute: "2-digit",
  });
}

function plural(n: number, word: string): string {
  return `${n} ${word}${n === 1 ? "" : "s"}`;
}

function summary(changes: ProductChange[]): string {
  const count = (c: ProductChange["change"]) => changes.filter((x) => x.change === c).length;
  return [
    [count("added"), "added"],
    [count("removed"), "removed"],
    [count("updated"), "updated"],
    [count("downgraded"), "downgraded"],
  ]
    .filter(([n]) => n)
    .map(([n, w], i) => (i === 0 ? `${plural(n as number, "plug-in")} ${w}` : `${n} ${w}`))
    .join(", ");
}

function Row({ dot, title, sub, right }: { dot: string; title: ReactNode; sub?: ReactNode; right: ReactNode }) {
  return (
    <div className="flex items-center gap-3 px-4 py-1.5">
      <span className={`h-1.5 w-1.5 shrink-0 rounded-full ${dot}`} aria-hidden="true" />
      <div className="min-w-0 flex-1">
        <div className="truncate">{title}</div>
        {sub && <div className="truncate text-xs text-dim">{sub}</div>}
      </div>
      {right}
    </div>
  );
}

function ScanRow({
  e,
  busy,
  open,
  onToggle,
  onGoBack,
  onConfirm,
  onCancel,
}: {
  e: Extract<HistoryEntry, { type: "scan" }>;
  busy: boolean;
  open: Open | null;
  onToggle: () => void;
  onGoBack: () => void;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  const title = e.first ? `First scan · ${plural(e.plugins, "plug-in")}` : summary(e.changes);
  return (
    <>
      <Row
        dot="bg-dim"
        title={
          e.changes.length > 0 ? (
            <button onClick={onToggle} className="hover:underline" aria-expanded={open?.kind === "details"}>
              {title}
            </button>
          ) : (
            title
          )
        }
        sub={`Scan · ${when(e.at)}`}
        right={
          e.latest ? (
            <span className="text-xs text-dim">Now</span>
          ) : (
            <Button variant="quiet" disabled={busy} onClick={onGoBack} title="See what going back to this scan takes">
              Go back…
            </Button>
          )
        }
      />
      {open?.kind === "details" && (
        <ul className="px-4 pb-2 pl-8 text-xs">
          {e.changes.map((c) => (
            <li key={c.product_id} className="flex gap-2 py-0.5">
              <span className="w-20 shrink-0 text-dim">{CHANGED[c.change]}</span>
              <span className="min-w-0 flex-1 truncate">
                {c.name}
                {c.vendor && <span className="ml-2 text-dim">{c.vendor}</span>}
              </span>
              <span className="font-mono text-[11px] text-dim">{versions(c)}</span>
            </li>
          ))}
        </ul>
      )}
      {open?.kind === "back" && <GoBackPanel plan={open.plan} busy={busy} onConfirm={onConfirm} onCancel={onCancel} />}
    </>
  );
}

const CHANGED: Record<ProductChange["change"], string> = {
  added: "Added",
  removed: "Removed",
  updated: "Updated",
  downgraded: "Downgraded",
};

function versions(c: ProductChange): string {
  if (c.change === "added") return c.to ?? "";
  if (c.change === "removed") return c.from ?? "";
  return `${c.from} → ${c.to}`;
}

/** What to do by hand to get a plug-in back to how it was then. */
function backStep(c: ProductChange): string {
  switch (c.change) {
    case "added":
      return `Reinstall ${c.name}${c.to ? ` ${c.to}` : ""}`;
    case "removed":
      return `Uninstall ${c.name}`;
    case "updated":
      return `Update ${c.name} to ${c.to}`;
    case "downgraded":
      return `Install ${c.name} ${c.to} (now ${c.from})`;
  }
}

function GoBackPanel({
  plan,
  busy,
  onConfirm,
  onCancel,
}: {
  plan: GoBackPlan;
  busy: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  const blocked = plan.files.some((f) => f.missing_backup);
  const nothing = plan.files.length === 0 && plan.plugins.length === 0;
  return (
    <div className="mx-4 mb-2 rounded-md border border-line bg-panel px-3 py-2 text-xs">
      <p className="font-medium">Back to how this machine was on {when(plan.at)}</p>
      {nothing && <p className="mt-1 text-dim">Nothing to do: this machine already matches that scan.</p>}
      {plan.files.length > 0 && (
        <>
          <p className="mt-2 text-dim">Patch Pilot puts these files back:</p>
          <FileList files={plan.files} />
        </>
      )}
      {plan.plugins.length > 0 && (
        <>
          <p className="mt-2 text-dim">
            Plug-ins to change yourself (Patch Pilot never installs or uninstalls them):
          </p>
          <ul className="mt-1">
            {plan.plugins.map((c) => (
              <li key={c.product_id} className="flex gap-2 py-0.5">
                <span className="text-dim" aria-hidden="true">
                  ☐
                </span>
                <span className="min-w-0 flex-1 truncate">{backStep(c)}</span>
                {c.vendor && <span className="text-dim">{c.vendor}</span>}
              </li>
            ))}
          </ul>
        </>
      )}
      <div className="mt-2 flex justify-end gap-2">
        <Button variant="quiet" onClick={onCancel}>
          {plan.files.length > 0 ? "Cancel" : "Close"}
        </Button>
        {plan.files.length > 0 && (
          <Button disabled={busy || blocked} onClick={onConfirm}>
            Put {plural(plan.files.length, "file")} back
          </Button>
        )}
      </div>
    </div>
  );
}

function FilesRow({
  e,
  busy,
  open,
  onUndo,
  onConfirm,
  onCancel,
}: {
  e: Extract<HistoryEntry, { type: "files" }>;
  busy: boolean;
  open: FilePreview[] | null;
  onUndo: () => void;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  const where = e.files.length === 1 ? `${e.files[0].folder}/${e.files[0].name}` : plural(e.files.length, "file");
  const blocked = open?.some((f) => f.missing_backup) ?? false;
  return (
    <>
      <Row
        dot={e.undone ? "bg-dim" : "bg-accent"}
        title={<span className={e.undone ? "text-dim" : undefined}>{e.label}</span>}
        sub={`${where} · ${when(e.at)}`}
        right={
          e.undone ? (
            <Badge>Undone</Badge>
          ) : (
            <Button variant="quiet" disabled={busy || open !== null} onClick={onUndo}>
              Undo
            </Button>
          )
        }
      />
      {open && (
        <div className="mx-4 mb-2 rounded-md border border-line bg-panel px-3 py-2 text-xs">
          <FileList files={open} />
          {open.some((f) => f.changed_since) && (
            <p className="mt-1.5 text-warn">
              Files marked "edited since" changed after this. Undo saves them first, so you can undo the undo.
            </p>
          )}
          <div className="mt-2 flex justify-end gap-2">
            <Button variant="quiet" onClick={onCancel}>
              Cancel
            </Button>
            <Button disabled={busy || blocked} onClick={onConfirm}>
              Undo change
            </Button>
          </div>
        </div>
      )}
    </>
  );
}

function FileList({ files }: { files: FilePreview[] }) {
  return (
    <ul className="mt-1">
      {files.map((f) => (
        <li key={f.path} className="flex items-center gap-2 py-0.5" title={f.path}>
          <span className="w-16 shrink-0 text-dim">{f.restore ? "Put back" : "Remove"}</span>
          <span className="min-w-0 flex-1 truncate">
            {f.name}
            <span className="ml-2 text-dim">{f.folder}</span>
          </span>
          {f.missing_backup ? (
            <span className="text-bad">saved copy missing</span>
          ) : (
            f.changed_since && <Badge tone="amber">edited since</Badge>
          )}
        </li>
      ))}
    </ul>
  );
}
