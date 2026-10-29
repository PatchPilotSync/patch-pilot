import { useState } from "react";
import {
  cleanUpDuplicates,
  findDuplicates,
  revealPath,
  SHOW_IN_FOLDER,
  type DuplicateGroup,
  type Duplicates as Found,
} from "./api";
import { Badge, Button, Folder, FolderControls, Tile, useFolders } from "./ui";

/**
 * Duplicate presets (decision #66): the same preset saved in several folders on this
 * machine, matched by contents. Suggests keeping the copy in the plug-in's own preset
 * folder; the user can pick another or keep every copy. Removed copies go into a
 * restore point, so History can put them back. Searches only when asked.
 */
export function Duplicates() {
  const [found, setFound] = useState<Found | null>(null);
  const [busy, setBusy] = useState<"find" | "clean" | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [done, setDone] = useState<string | null>(null);
  /** Group hash to the path to keep; absent means the suggested (first) copy. */
  const [keep, setKeep] = useState<Record<string, string>>({});
  /** Groups to leave alone. */
  const [skip, setSkip] = useState<Set<string>>(new Set());
  const folders = useFolders(null);

  async function find() {
    setBusy("find");
    setError(null);
    try {
      setFound(await findDuplicates());
      setKeep({});
      setSkip(new Set());
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
    }
  }

  const kept = (g: DuplicateGroup) => keep[g.hash] ?? g.copies[0].path;
  const chosen = (found?.groups ?? []).filter((g) => !skip.has(g.hash));
  const extra = chosen.reduce((n, g) => n + g.copies.length - 1, 0);
  const bytes = chosen.reduce((n, g) => n + g.size * (g.copies.length - 1), 0);

  async function clean() {
    setBusy("clean");
    setError(null);
    try {
      const r = await cleanUpDuplicates(
        chosen.map((g) => ({ keep: kept(g), remove: g.copies.map((c) => c.path).filter((p) => p !== kept(g)) })),
      );
      setDone(
        r.removed > 0
          ? `Removed ${count(r.removed, "copy", "copies")} (${size(r.freed)}). Undo in History.` +
              (r.skipped.length ? ` Left ${r.skipped.length} that changed since the search.` : "")
          : "Nothing removed: the copies changed since the search.",
      );
      setFound(await findDuplicates());
      setKeep({});
      setSkip(new Set());
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
    }
  }

  const groups = found?.groups ?? [];
  return (
    <section>
      {/* A row like the folders around it on This machine, with its buttons at the end. */}
      <div className="flex h-9 items-center gap-1.5 pl-3 pr-2 hover:bg-sel/60">
        <span className="w-2.5 shrink-0" aria-hidden="true" />
        <Tile name="copies" />
        <span className="shrink-0 font-medium">Duplicate presets</span>
        <span className="ml-1.5 min-w-0 flex-1 truncate text-[11px] text-dim">
          {found === null
            ? "The same preset saved in more than one folder, matched by contents"
            : groups.length === 0
              ? `No duplicates among ${count(found.files, "preset file")}`
              : `${count(extra, "extra copy", "extra copies")} · ${size(bytes)} · in ${count(groups.length, "group")}`}
        </span>
        <Button variant="link" onClick={find} disabled={busy !== null} className="h-6 shrink-0 px-2">
          {busy === "find" ? "Looking…" : found ? "Look again" : "Find"}
        </Button>
        {groups.length > 0 && extra > 0 && (
          <Button onClick={clean} disabled={busy !== null} className="h-6 shrink-0 whitespace-nowrap px-2.5">
            {busy === "clean" ? "Cleaning up…" : `Clean up ${extra}`}
          </Button>
        )}
      </div>
      {error && <p className="pl-16 pr-4 pt-1 text-[11px] text-bad">{error}</p>}
      {done && <p className="pl-16 pr-4 pt-1 text-[11px] text-ok">{done}</p>}
      {groups.length > 0 && (
        <div className="pb-1.5">
          <p className="pl-16 pr-4 pt-1 text-[11px] text-dim">
            Keeps the copy in the plug-in's own folder unless you pick another. Removed copies are saved first.
          </p>
          <FolderControls ids={groups.map((g) => g.hash)} folders={folders} />
          {groups.map((g) => {
            const off = skip.has(g.hash);
            return (
              <Folder
                key={g.hash}
                title={<span className={off ? "text-dim" : ""}>{g.name}</span>}
                count={g.copies.length}
                className="pr-4"
                indent={60}
                open={folders.isOpen(g.hash)}
                onToggle={() => folders.toggle(g.hash)}
                badges={<span className="font-mono text-[11px] text-dim">{size(g.size)}</span>}
                actions={
                  <label className="flex shrink-0 items-center gap-1 text-[11px] text-dim" title="Leave every copy">
                    <input
                      type="checkbox"
                      checked={off}
                      onChange={() =>
                        setSkip((s) => {
                          const next = new Set(s);
                          if (off) next.delete(g.hash);
                          else next.add(g.hash);
                          return next;
                        })
                      }
                    />
                    Keep all
                  </label>
                }
              >
                <ul className="pb-1">
                  {g.copies.map((c, i) => {
                    const isKept = kept(g) === c.path;
                    return (
                      <li key={c.path} className="group flex h-7 items-center gap-2 pl-20 pr-4 hover:bg-sel/60">
                        <input
                          type="radio"
                          name={g.hash}
                          aria-label={`Keep ${c.path}`}
                          checked={isKept}
                          disabled={off}
                          onChange={() => setKeep((k) => ({ ...k, [g.hash]: c.path }))}
                        />
                        <span className="w-32 shrink-0 truncate text-xs">{c.place}</span>
                        <span
                          className={`min-w-0 flex-1 truncate font-mono text-[11px] ${isKept || off ? "" : "text-dim line-through"}`}
                          title={c.path}
                        >
                          {c.path}
                        </span>
                        <Button
                          variant="link"
                          className="h-5 shrink-0 px-0 text-[11px] opacity-0 focus:opacity-100 group-hover:opacity-100"
                          onClick={() => revealPath(c.path).catch((e) => setError(String(e)))}
                        >
                          {SHOW_IN_FOLDER}
                        </Button>
                        {i === 0 && <Badge>suggested</Badge>}
                      </li>
                    );
                  })}
                </ul>
              </Folder>
            );
          })}
        </div>
      )}
    </section>
  );
}

function count(n: number, one: string, many = `${one}s`): string {
  return `${n.toLocaleString()} ${n === 1 ? one : many}`;
}

function size(bytes: number): string {
  if (bytes < 1024 * 1024) return `${Math.max(1, Math.round(bytes / 1024))} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}
