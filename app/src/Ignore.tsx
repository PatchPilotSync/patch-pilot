import { useEffect, useState } from "react";
import { ignoreItems, ignoreList, unignore, type IgnoreView, type Matcher } from "./api";
import { Button, Notice } from "./ui";

/**
 * Ignore (decision #65): stop asking for something on a machine. It drops out
 * of the score, Get Up To Speed, Bring over and every count; "N ignored" lists
 * what was ignored so it can be asked for again.
 */

/** A small "Ignore" for a row or folder header; shows on hover so lists stay quiet. */
export function IgnoreButton({
  onIgnore,
  title = "Stop asking for this on this machine. It no longer counts against the score.",
  disabled,
  reveal = "row",
}: {
  onIgnore: () => void;
  title?: string;
  disabled?: boolean;
  /**
   * Show on hover of the row (the nearest `group`), or of the whole checklist
   * step (`group/step`), so a step's lines all offer it at once.
   */
  reveal?: "row" | "step";
}) {
  // One click per button: a second click while the row goes away would ignore twice.
  const [clicked, setClicked] = useState(false);
  return (
    <button
      type="button"
      title={title}
      disabled={disabled}
      onClick={(e) => {
        e.preventDefault();
        e.stopPropagation();
        if (clicked) return;
        setClicked(true);
        setTimeout(() => setClicked(false), 2000);
        onIgnore();
      }}
      className={`h-5 shrink-0 rounded px-1.5 text-[11px] text-dim opacity-0 hover:bg-sel hover:text-fg focus-visible:opacity-100 disabled:opacity-0 ${
        reveal === "step" ? "group-hover/step:opacity-100" : "group-hover:opacity-100"
      }`}
    >
      Ignore
    </button>
  );
}

/** "3 ignored": opens the list of what was ignored, each with Un-ignore and where it applies. */
export function IgnoredToggle({
  ignores,
  machine,
  onError,
  what = (v) => (v.hidden ? `${v.hidden} item${v.hidden === 1 ? "" : "s"}` : ""),
  listed = false,
}: {
  ignores: IgnoreView[];
  /** The machine they're on; null for this one. */
  machine: string | null;
  onError: (e: string) => void;
  /** How much each one hides, e.g. "12 files". */
  what?: (v: IgnoreView) => string;
  /** Always open, with no "N ignored" toggle (Settings). */
  listed?: boolean;
}) {
  const [open, setOpen] = useState(listed);
  const [busy, setBusy] = useState(false);
  if (!ignores.length) return null;
  const run = (p: Promise<void>) => {
    setBusy(true);
    p.catch((e) => onError(String(e))).finally(() => setBusy(false));
  };
  return (
    <div className="text-[11px]">
      {!listed && (
        <button
          type="button"
          onClick={() => setOpen((o) => !o)}
          aria-expanded={open}
          className="text-dim underline-offset-2 hover:text-fg hover:underline"
        >
          {ignores.length} ignored{open ? "" : "…"}
        </button>
      )}
      {open && (
        <ul className={`${listed ? "" : "mt-1 "}flex flex-col`}>
          {ignores.map((v) => (
            <li key={v.key} className="flex h-7 items-center gap-2 rounded-[5px] px-1 hover:bg-sel/50">
              <span className="min-w-0 flex-1 truncate text-xs" title={v.label}>
                {v.label}
                {what(v) && <span className="ml-2 text-dim">{what(v)}</span>}
              </span>
              <select
                aria-label="Where it's ignored"
                disabled={busy}
                value={v.every_machine ? "all" : "here"}
                onChange={(e) =>
                  run(ignoreItems(machine, v.label, rulesOf(v), e.target.value === "all"))
                }
                className="h-6 rounded-md border border-line bg-canvas px-1 text-[11px] text-dim"
              >
                <option value="here">{machine ? "That machine" : "This machine"}</option>
                <option value="all">Every machine</option>
              </select>
              <Button variant="quiet" className="h-6 px-2" disabled={busy} onClick={() => run(unignore(machine, v.key))}>
                Un-ignore
              </Button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

/**
 * "2 things ignored on this machine" as a {@link Notice} line among a screen's
 * others (decision #109), opening to the list to un-ignore. Nothing when none.
 */
export function IgnoredNotice({
  ignores,
  machine,
  name,
  onError,
}: {
  ignores: IgnoreView[];
  /** The machine they're on; null for this one. */
  machine: string | null;
  /** That machine's name, for the line. */
  name?: string;
  onError: (e: string) => void;
}) {
  const [open, setOpen] = useState(false);
  if (!ignores.length) return null;
  const n = ignores.length;
  return (
    <Notice
      tone="dim"
      icon="hidden"
      text={
        <span className="text-dim">
          {n} thing{n === 1 ? "" : "s"} ignored on {machine ? (name ?? "that machine") : "this machine"}. Left out of
          the score and the checklist.
        </span>
      }
      open={open}
      onToggle={() => setOpen((o) => !o)}
    >
      <div className="-mx-2 -my-1">
        <IgnoredToggle ignores={ignores} machine={machine} onError={onError} listed />
      </div>
    </Notice>
  );
}

/** The ignores on a machine (this one when null), reloaded when `refresh` changes. */
export function useIgnores(machine: string | null, refresh: unknown): IgnoreView[] {
  const [ignores, setIgnores] = useState<IgnoreView[]>([]);
  useEffect(() => {
    let current = true;
    ignoreList(machine).then(
      (v) => current && setIgnores(v),
      () => {},
    );
    return () => {
      current = false;
    };
  }, [machine, refresh]);
  return ignores;
}

/** An ignore's rules, which its key is (as JSON). */
function rulesOf(v: IgnoreView): Matcher[] {
  try {
    return JSON.parse(v.key) as Matcher[];
  } catch {
    return [];
  }
}

/** Ignore these products ("Serum", or a step's products). */
export const productRules = (ids: string[]): Matcher[] => [...new Set(ids)].map((product) => ({ product }));

/**
 * Rules for a folder of files (`root`/`rel`): one `ROOT/<folder>/**` per root
 * when they share a folder, else each file. `whole` ignores their roots entirely
 * (an app's group).
 */
export function pathRules(files: { root: string; rel: string }[], whole = false): Matcher[] {
  const byRoot = new Map<string, string[]>();
  for (const f of files) byRoot.set(f.root, [...(byRoot.get(f.root) ?? []), f.rel]);
  const out: Matcher[] = [];
  for (const [root, rels] of byRoot) {
    if (whole) {
      out.push({ path_glob: `${root}/**` });
      continue;
    }
    const dir = commonDir(rels);
    if (dir) out.push({ path_glob: `${root}/${dir}/**` });
    else for (const rel of new Set(rels)) out.push({ path_glob: `${root}/${rel}` });
  }
  return out;
}

/** The deepest folder holding every path, "" when they share none. */
function commonDir(paths: string[]): string {
  if (!paths.length) return "";
  let dirs = paths[0].split("/").slice(0, -1);
  for (const p of paths.slice(1)) {
    const d = p.split("/").slice(0, -1);
    let n = 0;
    while (n < dirs.length && n < d.length && dirs[n].toLowerCase() === d[n].toLowerCase()) n++;
    dirs = dirs.slice(0, n);
  }
  return dirs.join("/");
}
