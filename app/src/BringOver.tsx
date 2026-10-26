import { useCallback, useEffect, useMemo, useState, type ReactNode } from "react";
import { bringFiles, fileKey, filesToBring, ignoreItems, type BringResult, type BringView, type FileToBring } from "./api";
import { bringSummary, isReaperScript, isTemplate, Needs } from "./bring";
import { IgnoreButton, IgnoredToggle, pathRules } from "./Ignore";
import { Badge, Button, buildTree, Card, Folder, FolderTree, isConflictCopy, treeItems, useFolders } from "./ui";

/**
 * Bring over (decision #45): the Primary Rig's templates, scripts, themes and
 * plug-in presets (decision #63) this machine lacks, grouped by app. One
 * click brings the ticked ones over, each with every file it needs (decision
 * #62); anything replaced is backed up first. Anything can be ignored here
 * (decision #65), and then stops being listed or counted. Loads its own list,
 * so any screen about this machine can show it.
 */
export function BringOver({ onError }: { onError: (e: string) => void }) {
  const [view, setView] = useState<BringView | null>(null);
  const [skipped, setSkipped] = useState<Set<string>>(new Set());
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<BringResult | null>(null);
  const [leftOut, setLeftOut] = useState(0);

  const files = useMemo(() => view?.files ?? [], [view]);
  const needs = useMemo(() => new Needs(files), [files]);
  const load = useCallback(() => {
    filesToBring().then(
      (v) => {
        setView(v);
        // Sync-app conflict copies, and anything needing one, start unticked.
        setSkipped(new Needs(v.files).withNeededBy(v.files.filter((f) => isConflictCopy(f.rel)).map(fileKey)));
      },
      (e) => onError(String(e)),
    );
  }, [onError]);
  useEffect(load, [load]);

  const canBring = (f: FileToBring) => !f.incomplete;
  const isChosen = (f: FileToBring) => canBring(f) && !skipped.has(fileKey(f));
  const chosen = files.filter(isChosen);
  const groups = useMemo(() => groupByApp(files), [files]);
  // One app opens by default; its kinds (Scripts, FX chains, ...) stay folded.
  const folders = useFolders("bring-over", (id) => !id.includes("/") && groups.length === 1);

  if (!view || !view.has_source) return null;
  if (files.length === 0 && !result && !view.linked) return null;

  // The catch-all: every Reaper script here, whatever is ticked.
  const scripts = files.filter(isReaperScript);
  const allScripts = scripts.filter((f) => canBring(f) && !isConflictCopy(f.rel));
  // And every template (Reaper and Ableton).
  const templates = files.filter(isTemplate);
  const allTemplates = templates.filter((f) => canBring(f) && !isConflictCopy(f.rel));
  const presets = files.filter(isPreset);
  const allPresets = presets.filter((f) => canBring(f) && !isConflictCopy(f.rel));

  async function bring(which: FileToBring[] = chosen, skipped = 0) {
    setBusy(true);
    try {
      setResult(await bringFiles(which.map(fileKey)));
      setLeftOut(skipped);
      load();
    } catch (e) {
      onError(String(e));
    } finally {
      setBusy(false);
    }
  }

  /** Tick or untick `fs` as one: ticking brings what they need along, unticking leaves what needs them. */
  const setChosen = (fs: FileToBring[], on: boolean) =>
    setSkipped((s) => {
      const n = new Set(s);
      const keys = fs.filter(canBring).map(fileKey);
      if (on) for (const k of needs.withNeeds(keys)) n.delete(k);
      else for (const k of needs.withNeededBy(keys)) n.add(k);
      return n;
    });

  /** Stop asking for `fs` here, with whatever needs them (it wouldn't work without them). */
  const ignore = (label: string, fs: FileToBring[], whole = false) => {
    const keys = new Set(fs.map(fileKey));
    const needing = files.filter((f) => !keys.has(fileKey(f)) && needs.withNeededBy(keys).has(fileKey(f)));
    const rules = [...pathRules(fs, whole), ...needing.map((f) => ({ path_glob: `${f.root}/${f.rel}` }))];
    setBusy(true);
    ignoreItems(null, label, rules)
      .then(load, (e) => onError(String(e)))
      .finally(() => setBusy(false));
  };
  const ignoreAction = (label: string, fs: FileToBring[], whole = false) => (
    <IgnoreButton
      disabled={busy}
      title="Stop asking to bring these over to this machine. They no longer count against its score."
      onIgnore={() => ignore(label, fs, whole)}
    />
  );

  const upToDate = files.length === 0;
  const summary = result && bringSummary(result, "the Primary machine", leftOut);

  const summaryLine = groups.map(([app, fs]) => `${app}: ${summaryOf(fs)}`).join(" · ");
  return (
    <Card
      icon={
        upToDate && (
          <svg width="14" height="14" viewBox="0 0 14 14" aria-hidden="true" className="shrink-0 text-ok">
            <circle cx="7" cy="7" r="6.25" fill="none" stroke="currentColor" strokeWidth="1.3" />
            <path d="M4.2 7.2 6.1 9 9.8 5.2" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" />
          </svg>
        )
      }
      title={
        upToDate ? <span className="text-ok">Up to date with the Primary Rig</span> : "Bring over from the Primary Rig"
      }
      sub={
        <span title={upToDate ? undefined : summaryLine}>
          {upToDate
            ? view.ignored.length
              ? "Everything from the Primary Rig you haven't ignored is on this machine."
              : "Every template, script and theme from the Primary Rig is on this machine."
            : summaryLine}
        </span>
      }
      actions={
        <>
          {!upToDate && (
            <Button variant="quiet" onClick={() => setOpen((o) => !o)} className="shrink-0">
              {open ? "Hide files" : "Show files"}
            </Button>
          )}
          {view.linked && allScripts.length > 0 && allScripts.length < files.length && (
            <Button
              variant="link"
              onClick={() => bring(allScripts, scripts.filter((f) => f.incomplete).length)}
              disabled={busy}
              className="shrink-0 whitespace-nowrap"
              title="Every Reaper script, ReaPack package and the files they need, whatever is ticked"
            >
              All {allScripts.length} scripts
            </Button>
          )}
          {view.linked && allTemplates.length > 0 && allTemplates.length < files.length && (
            <Button
              variant="link"
              onClick={() => bring(allTemplates, templates.filter((f) => f.incomplete).length)}
              disabled={busy}
              className="shrink-0 whitespace-nowrap"
              title="Every Reaper track and project template and Ableton template, whatever is ticked"
            >
              All {allTemplates.length} templates
            </Button>
          )}
          {view.linked && allPresets.length > 0 && allPresets.length < files.length && (
            <Button
              variant="link"
              onClick={() => bring(allPresets, presets.filter((f) => f.incomplete).length)}
              disabled={busy}
              className="shrink-0 whitespace-nowrap"
              title="Every plug-in preset, whatever is ticked"
            >
              All {allPresets.length} presets
            </Button>
          )}
          {view.linked && chosen.length > 0 && (
            <Button onClick={() => bring()} disabled={busy} className="shrink-0 whitespace-nowrap">
              {busy ? "Bringing over…" : `Bring over ${chosen.length}`}
            </Button>
          )}
        </>
      }
    >
      {!view.linked && files.length > 0 && (
        <p className="mt-1.5 text-[11px] text-dim">
          Link this machine with the Primary one (Link a machine, top bar) to bring these over.
        </p>
      )}

      {view.ignores.length > 0 && (
        <div className="mt-1">
          <IgnoredToggle
            ignores={view.ignores}
            machine={null}
            onError={onError}
            what={(v) => `${v.files} file${v.files === 1 ? "" : "s"}`}
          />
        </div>
      )}

      {summary && (
        <p className="mt-1.5 text-[11px] text-ok">
          {summary.text}
          {summary.warn && <span className="text-warn"> {summary.warn}</span>}
        </p>
      )}

      {open && !upToDate && (
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
                actions={
                  <>
                    {ignoreAction(`All ${app} files`, fs, true)}
                    {view.linked && <FolderTick files={fs} isChosen={isChosen} onChange={setChosen} />}
                  </>
                }
              >
                {groupByKind(fs).map(([kind, ks]) => {
                  const id = `${app}/${kind}`;
                  const ticked = ks.filter(isChosen).length;
                  return (
                    <Folder
                      key={id}
                      title={isPreset(ks[0]) ? kind : plural(kind)}
                      count={ks.length}
                      className="pl-5 pr-1 text-xs"
                      open={folders.isOpen(id)}
                      onToggle={() => folders.toggle(id)}
                      badges={view.linked && ticked < ks.length && <Badge>{ticked} ticked</Badge>}
                      actions={
                        <>
                          {ignoreAction(`${app} ${plural(lower(kind))}`, ks)}
                          {view.linked && <FolderTick files={ks} isChosen={isChosen} onChange={setChosen} />}
                        </>
                      }
                    >
                      <FolderTree
                        node={buildTree(ks, dirsOf, id)}
                        folders={folders}
                        indent={36}
                        actions={(node) => (
                          <>
                            {ignoreAction(node.name, treeItems(node))}
                            {view.linked && (
                              <FolderTick files={treeItems(node)} isChosen={isChosen} onChange={setChosen} />
                            )}
                          </>
                        )}
                        renderItems={(items, indent) => (
                          <ul className="flex flex-col" style={{ paddingLeft: indent }}>
                            {items.map((f) => (
                              <FileLine
                                key={fileKey(f)}
                                f={f}
                                linked={view.linked}
                                cloud={view.cloud}
                                checked={isChosen(f)}
                                companions={needs.withNeeds([fileKey(f)]).size - 1}
                                onToggle={() => setChosen([f], !isChosen(f))}
                                ignore={ignoreAction(fileName(f.rel), [f])}
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
            Files that work together are ticked together, so they arrive whole. Anything replaced is saved first.
          </p>
        </div>
      )}
    </Card>
  );
}

/** A folder header's checkbox: ticked when every file in it is, dashed when some are. */
export function FolderTick({
  files,
  isChosen,
  onChange,
}: {
  files: FileToBring[];
  isChosen: (f: FileToBring) => boolean;
  onChange: (files: FileToBring[], on: boolean) => void;
}) {
  const can = files.filter((f) => !f.incomplete);
  const n = can.filter(isChosen).length;
  return (
    <input
      type="checkbox"
      className="mr-1 shrink-0 accent-accent"
      aria-label={n === can.length ? "Untick every file in this folder" : "Tick every file in this folder"}
      title={n === can.length ? "Untick every file in this folder" : "Tick every file in this folder"}
      disabled={can.length === 0}
      checked={can.length > 0 && n === can.length}
      ref={(el) => {
        if (el) el.indeterminate = n > 0 && n < can.length;
      }}
      onChange={() => onChange(can, n < can.length)}
    />
  );
}

function FileLine({
  f,
  linked,
  cloud,
  checked,
  companions,
  onToggle,
  ignore,
}: {
  f: FileToBring;
  linked: boolean;
  cloud: string | null;
  checked: boolean;
  /** Other files it brings along. */
  companions: number;
  onToggle: () => void;
  ignore: ReactNode;
}) {
  return (
    <li>
      <label className="group flex items-center gap-2 rounded-[5px] px-1 py-1 hover:bg-sel/50">
        <input
          type="checkbox"
          className="accent-accent"
          disabled={f.incomplete || !linked}
          checked={checked}
          onChange={onToggle}
        />
        <span className="min-w-0 flex-1 truncate text-xs" title={f.rel}>
          {fileName(f.rel)}
        </span>
        {companions > 0 && (
          <span
            className="shrink-0 text-[11px] text-dim"
            title="Files it needs to work. They're brought over with it."
          >
            + {companions} file{companions === 1 ? "" : "s"}
          </span>
        )}
        {isConflictCopy(f.rel) && (
          <span title="A copy a sync app (Dropbox, OneDrive...) made when two machines changed this file at once. Not ticked, so it stays behind.">
            <Badge tone="amber">conflict copy</Badge>
          </span>
        )}
        {f.too_large && !f.incomplete ? (
          <span
            title={`Over 1.9 MB, too big for the link: it comes through ${cloud}, as soon as Patch Pilot on the machine that has it sends it.`}
          >
            <Badge>Via {cloud}</Badge>
          </span>
        ) : f.too_large ? (
          <span title="Over the 2 MB the link can carry. Pick a cloud folder under Big files to bring it over through Dropbox, OneDrive or similar.">
            <Badge>Too large to send</Badge>
          </span>
        ) : f.incomplete ? (
          <span title="A file it needs is too large for the link, so it would arrive without it. Pick a cloud folder under Big files to bring it over.">
            <Badge>Needs a file too large to send</Badge>
          </span>
        ) : (
          f.replaces && <Badge tone="amber">Updates this machine's copy</Badge>
        )}
        {ignore}
      </label>
    </li>
  );
}

function groupByKind(files: FileToBring[]): [string, FileToBring[]][] {
  const m = new Map<string, FileToBring[]>();
  for (const f of files) m.set(f.kind, [...(m.get(f.kind) ?? []), f]);
  return [...m.entries()];
}

export function groupByApp(files: FileToBring[]): [string, FileToBring[]][] {
  const m = new Map<string, FileToBring[]>();
  for (const f of files) m.set(f.app, [...(m.get(f.app) ?? []), f]);
  return [...m.entries()];
}

/** Presets group by their vendor's folder, the rest by kind. */
const isPreset = (f: FileToBring) => f.root.startsWith("PRESETS_");

/** Folders to show a file in: below its kind's folder, or for a preset, its whole path. */
function dirsOf(f: FileToBring): string[] {
  return f.rel.split("/").slice(isPreset(f) ? 0 : 1, -1);
}

/** "3 FX chains, 12 scripts"; for presets "14 presets". */
export function summaryOf(files: FileToBring[]): string {
  if (files.every(isPreset)) return `${files.length} preset${files.length === 1 ? "" : "s"}`;
  const counts = new Map<string, number>();
  for (const f of files) counts.set(f.kind, (counts.get(f.kind) ?? 0) + 1);
  return [...counts.entries()].map(([k, n]) => `${n} ${n === 1 ? lower(k) : plural(lower(k))}`).join(", ");
}

/** "Track template" → "track template", but "FX chain" stays. */
function lower(s: string): string {
  return s.length > 1 && s[1] === s[1].toLowerCase() ? s[0].toLowerCase() + s.slice(1) : s;
}

function plural(s: string): string {
  return s.endsWith("s") || s.endsWith("data") ? s : `${s}s`;
}

function fileName(rel: string): string {
  return rel.slice(rel.lastIndexOf("/") + 1);
}
