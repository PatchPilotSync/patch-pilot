import { useCallback, useEffect, useMemo, useState, type ReactNode } from "react";
import {
  backUpNow,
  backupRestoreList,
  backupView,
  fileKey,
  onBackupChanged,
  onSyncChanged,
  restoreBackup,
  setBackupAuto,
  setBackupFolder,
  setKeepInSync,
  settleConflict,
  syncLookNow,
  syncView,
  type BackupSet,
  type BackupView,
  type FileToBring,
  type RestoreReport,
  type RestoreView,
  type SyncConflict,
  type SyncSource,
  type SyncView,
} from "./api";
import { FolderPicker } from "./BigFiles";
import { groupByApp, summaryOf } from "./BringOver";
import { Button, Notice, timeAgo } from "./ui";

/**
 * Sync and backup (decisions #67 and #70): one line on This machine. Once a
 * cloud folder is picked, new and changed templates, scripts and presets keep
 * themselves in sync with linked machines through it, and every change is
 * backed up there. Show opens the folder, the switches and Restore. On a
 * fresh machine that finds another machine's backup, the line offers to
 * restore it. `openAt` opens the line (the tray's Keep Files in Sync with no
 * folder picked yet).
 */
export function BackupLine({
  onError,
  fresh,
  openAt,
}: {
  onError: (e: string) => void;
  fresh: boolean;
  openAt?: number;
}) {
  const [view, setView, load] = useBackupView(onError);
  const [sync, setSync] = useSyncView(onError, view);
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    if (openAt) setOpen(true);
  }, [openAt]);

  if (!view || !sync) return null;

  const mine = view.sets.find((s) => s.machine_id === view.this_machine);
  const others = view.sets.filter((s) => s.machine_id !== view.this_machine && s.backups.length > 0);
  const where = placeName(view);
  const latest = mine?.backups[0];
  const syncing = view.settings.keep_in_sync;
  const offerRestore = fresh && others.length > 0 && !latest;

  async function now() {
    setBusy(true);
    try {
      await backUpNow();
      load();
    } catch (e) {
      onError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function check() {
    setBusy(true);
    try {
      setSync(await syncLookNow());
    } catch (e) {
      onError(String(e));
    } finally {
      setBusy(false);
    }
  }

  let tone: Tone = "ok";
  let text: ReactNode;
  if (!view.settings.folder) {
    tone = "accent";
    text = fresh
      ? "Keep your files in sync through a cloud folder, or restore them from a backup."
      : "Keep templates, scripts and presets in sync between your machines through a cloud folder.";
  } else if (!view.dir) {
    tone = "warn";
    text = "The cloud folder isn't there now (signed out, or not connected).";
  } else if (view.error) {
    tone = "warn";
    text = `The last backup didn't finish: ${view.error}`;
  } else if (offerRestore) {
    tone = "accent";
    text = `A backup of ${others[0].machine_name} from ${when(others[0].backups[0].made_at)} is in ${where}. Restore it here?`;
  } else if (syncing) {
    [tone, text] = syncLine(sync, where);
  } else if (latest) {
    text = `Backed up ${timeAgo(latest.made_at)} to ${where}${view.settings.auto ? ", after every change" : ""}. Syncing is off.`;
  } else {
    tone = "accent";
    text = `Not backed up yet. Backups go to ${where}.`;
  }

  return (
    <>
      <Notice
        tone={tone}
        text={
          <span className="text-dim" title={view.dir ?? undefined}>
            <span className="font-medium text-fg">Sync &amp; backup</span> · {text}
          </span>
        }
        actions={
          view.dir &&
          !offerRestore &&
          (syncing ? (
            // Looks on its own too; this is for "why hasn't it come over yet?".
            <Button variant="link" onClick={check} disabled={busy} className="h-6 shrink-0 px-2">
              {busy ? "Checking…" : "Check now"}
            </Button>
          ) : (
            // Backups run on their own while syncing; the button is for by-hand backups.
            <Button variant="link" onClick={now} disabled={busy || view.running} className="h-6 shrink-0 px-2">
              {busy || view.running ? "Backing up…" : "Back up now"}
            </Button>
          ))
        }
        open={open}
        onToggle={() => setOpen((o) => !o)}
        toggle={[view.settings.folder ? (offerRestore ? "Restore…" : "Show") : "Set up", "Hide"]}
      >
        <Details view={view} onView={setView} sync={sync} onSync={setSync} onError={onError} />
      </Notice>
      <Conflicts sync={sync} onSync={setSync} onError={onError} />
    </>
  );
}

type Tone = "ok" | "accent" | "warn";

/**
 * The Sync & backup line while syncing: what the other machines' backups
 * brought (or why nothing came yet), so a machine behind the Primary Rig
 * can see it catching up.
 */
function syncLine(sync: SyncView, where: string): [Tone, string] {
  const last = sync.last;
  if (sync.linked === 0) return ["accent", `Backed up to ${where}. Link another machine to keep files in sync with it.`];
  if (last && last.waiting.length > 0)
    return ["warn", `New files are waiting: ${last.waiting.join("; ")}. Trying again shortly.`];
  const sources = (last?.sources ?? []).filter((s) => s.linked);
  if (!last) return ["ok", `In sync through ${where}. Checking your other machines' files…`];
  if (sources.length === 0)
    return ["accent", `No other machine's files in ${where} yet. Turn on Keep Files in Sync on your other machine too.`];
  const names = (f: (s: SyncSource) => boolean) => sources.filter(f).map((s) => s.machine_name).join(", ");
  const downloading = sources.reduce((a, s) => a + s.downloading, 0);
  if (downloading > 0)
    return ["accent", `Bringing over ${plural(downloading, "file")} from ${names((s) => s.downloading > 0)} as ${where} downloads them…`];
  const placed = sources.reduce((a, s) => a + s.placed, 0);
  if (placed > 0)
    return ["ok", `Brought over ${plural(placed, "file")} from ${names((s) => s.placed > 0)}. Undo in History.`];
  return ["ok", `In sync with ${names(() => true)} through ${where}, checked ${timeAgo(last.looked_at)}.`];
}

/** Whether files are kept in sync, reloaded when that or the backup changes. */
function useSyncView(onError: (e: string) => void, backup: BackupView | null) {
  const [view, setView] = useState<SyncView | null>(null);
  const load = useCallback(() => {
    syncView().then(setView, (e) => onError(String(e)));
  }, [onError]);
  useEffect(() => {
    load();
    const unlisten = onSyncChanged(load);
    return () => {
      unlisten.then((f) => f());
    };
  }, [load, backup]);
  return [view, setView] as const;
}

/**
 * Files changed here and on another machine: nothing is replaced until the
 * user picks a copy, and no conflicted-copy files are made. The other copy
 * stays in that machine's backup folder meanwhile.
 */
function Conflicts({
  sync,
  onSync,
  onError,
}: {
  sync: SyncView;
  onSync: (v: SyncView) => void;
  onError: (e: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const list = sync.conflicts;
  if (list.length === 0) return null;

  async function settle(c: SyncConflict, theirs: boolean) {
    setBusy(true);
    try {
      onSync(await settleConflict(c, theirs));
    } catch (e) {
      onError(String(e));
    } finally {
      setBusy(false);
    }
  }

  const machines = [...new Set(list.map((c) => c.machine_name))];
  return (
    <Notice
      tone="warn"
      text={`${plural(list.length, "file")} changed here and on ${machines.length === 1 ? machines[0] : "another machine"}. Pick which copy to keep.`}
      open={open}
      onToggle={() => setOpen((o) => !o)}
    >
      <ul className="flex flex-col">
        {list.map((c) => (
          <li key={`${c.machine_id}/${c.root}/${c.rel}`} className="flex items-center gap-2 py-0.5 text-xs">
            <span className="min-w-0 flex-1 truncate" title={c.rel}>
              {c.rel.slice(c.rel.lastIndexOf("/") + 1)}
              <span className="text-dim"> · {c.kind}</span>
            </span>
            <Button variant="link" className="h-6 shrink-0 px-2" disabled={busy} onClick={() => settle(c, true)}>
              Use {c.machine_name}&apos;s
            </Button>
            <Button variant="quiet" className="h-6 shrink-0 px-2" disabled={busy} onClick={() => settle(c, false)}>
              Keep this one
            </Button>
          </li>
        ))}
      </ul>
      <p className="mt-1 text-[11px] text-dim">
        Using the other copy saves this one first, so History can undo it. Keeping this one sends it to your other
        machines.
      </p>
    </Notice>
  );
}

/** The backup view, reloaded after each backup. */
function useBackupView(onError: (e: string) => void) {
  const [view, setView] = useState<BackupView | null>(null);
  const load = useCallback(() => {
    backupView().then(setView, (e) => onError(String(e)));
  }, [onError]);
  useEffect(() => {
    load();
    const unlisten = onBackupChanged(load);
    return () => {
      unlisten.then((f) => f());
    };
  }, [load]);
  return [view, setView, load] as const;
}

/** Set up's "Restore from a backup": pick the folder, then what to restore. */
export function RestoreFromBackup({ onError }: { onError: (e: string) => void }) {
  const [view, setView] = useBackupView(onError);
  if (!view) return null;
  return <Details view={view} onView={setView} onError={onError} />;
}

function Details({
  view,
  onView,
  sync,
  onSync,
  onError,
}: {
  view: BackupView;
  onView: (v: BackupView) => void;
  /** Absent in Set up's restore, which only restores. */
  sync?: SyncView;
  onSync?: (v: SyncView) => void;
  onError: (e: string) => void;
}) {
  const [picking, setPicking] = useState(false);
  const [busy, setBusy] = useState(false);

  async function act(p: Promise<BackupView>) {
    setBusy(true);
    try {
      onView(await p);
      setPicking(false);
    } catch (e) {
      onError(String(e));
    } finally {
      setBusy(false);
    }
  }

  // Picking the first folder here turns syncing on: that's what it's for.
  async function choose(service: string | null, folder: string | null) {
    const first = !view.settings.folder;
    await act(
      setBackupFolder(service, folder).then(async (v) => {
        if (first && sync && onSync && v.dir) {
          onSync(await setKeepInSync(true));
          return backupView();
        }
        return v;
      }),
    );
  }

  async function toggleSync(on: boolean) {
    if (!onSync) return;
    setBusy(true);
    try {
      onSync(await setKeepInSync(on));
      onView(await backupView());
    } catch (e) {
      onError(String(e));
    } finally {
      setBusy(false);
    }
  }

  if (!view.dir || picking) {
    return (
      <div>
        <FolderPicker
          clouds={view.clouds}
          busy={busy}
          missing={view.settings.folder !== null && !view.dir}
          preferred={view.settings.folder ?? view.transfer_folder}
          onChoose={choose}
          onCancel={view.dir ? () => setPicking(false) : undefined}
          hint={sync ? <SyncHint /> : <BackupHint />}
        />
      </div>
    );
  }

  const last = view.last;
  return (
    <div className="flex flex-col gap-1.5">
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-[11px] text-dim">
        {sync && (
          <label className="flex items-center gap-1.5" title="Settings, deletions and plug-ins never sync on their own.">
            <input
              type="checkbox"
              className="accent-accent"
              checked={view.settings.keep_in_sync}
              disabled={busy}
              onChange={(e) => toggleSync(e.target.checked)}
            />
            Keep new and changed files in sync with linked machines
          </label>
        )}
        {/* Syncing backs up after every change anyway. */}
        {!view.settings.keep_in_sync && (
          <label className="flex items-center gap-1.5">
            <input
              type="checkbox"
              className="accent-accent"
              checked={view.settings.auto}
              disabled={busy}
              onChange={(e) => act(setBackupAuto(e.target.checked))}
            />
            Back up after every scan that finds a change
          </label>
        )}
        <span className="min-w-0 flex-1 truncate" title={view.dir}>
          {view.dir}
        </span>
        <Button variant="quiet" className="h-6 px-2" onClick={() => setPicking(true)}>
          Change folder
        </Button>
      </div>
      {last && (
        <p className="text-[11px] text-dim">
          {last.unchanged
            ? `Nothing changed since the last backup (${plural(last.files, "file")}).`
            : `Last backup copied ${plural(last.copied, "changed file")}${last.versioned ? ` and kept ${plural(last.versioned, "earlier copy", "earlier copies")}` : ""}.`}
          {last.skipped.length > 0 && (
            <span className="text-warn" title={last.skipped.join("\n")}>
              {" "}
              {plural(last.skipped.length, "file")} skipped (too big, or couldn't be read).
            </span>
          )}
        </p>
      )}
      {sync?.on && <SyncSummary sync={sync} />}
      <Restore sets={view.sets} thisMachine={view.this_machine} onError={onError} />
    </div>
  );
}

function BackupHint() {
  return (
    <>
      Pick where backups go. Patch Pilot keeps them in a <span className="font-medium">Patch Pilot Backup</span> folder
      there, one folder per machine, and never deletes them.
    </>
  );
}

/** What picking a folder for syncing does, in the line above the picker. */
export function SyncHint() {
  return (
    <>
      Which cloud service do you use? Pick the same one on each machine. New and changed templates, scripts and presets
      go through a <span className="font-medium">Patch Pilot Backup</span> folder there and land on your other machines
      on their own.
    </>
  );
}

/** Each other machine's latest backup, as this machine sees it. */
function SyncSummary({ sync }: { sync: SyncView }) {
  const sources = sync.last?.sources ?? [];
  return (
    <div className="flex flex-col gap-0.5 text-[11px] text-dim">
      {sources.map((s) => (
        <p key={s.machine_id} className="truncate">
          <span className="font-medium text-fg">{s.machine_name}</span> ·{" "}
          {s.linked
            ? [
                `backed up ${timeAgo(s.made_at)}, ${plural(s.files, "file")}`,
                `${s.same.toLocaleString()} the same here`,
                s.placed > 0 && `${s.placed.toLocaleString()} brought over`,
                s.to_choose > 0 && `${s.to_choose.toLocaleString()} to choose`,
                s.downloading > 0 && `${s.downloading.toLocaleString()} downloading`,
              ]
                .filter(Boolean)
                .join(", ")
            : "not linked to this machine, so left alone"}
        </p>
      ))}
      <p>
        When two machines first sync, the Primary Rig's copy of a file wins. After that, new and changed files travel
        both ways. Deleted files and settings (key maps, Live's preferences) never sync on their own.
      </p>
      {sync.error && <p className="text-warn">{sync.error}</p>}
    </div>
  );
}

/**
 * Restore a backup onto this machine: pick a machine's backup and when, then
 * the apps to restore. Paths are mapped for this OS the way Bring over does,
 * and every write is a restore point History can undo.
 */
export function Restore({
  sets,
  thisMachine,
  onError,
}: {
  sets: BackupSet[];
  thisMachine: string | null;
  onError: (e: string) => void;
}) {
  const choices = useMemo(
    () =>
      sets.flatMap((s) =>
        s.backups.map((b, i) => ({
          id: `${s.folder}\n${b.stamp}`,
          set: s,
          stamp: b.stamp,
          label: `${s.machine_id === thisMachine ? "This machine" : s.machine_name} · ${when(b.made_at)}${i === 0 ? " (latest)" : ""} · ${plural(b.files, "file")}`,
        })),
      ),
    [sets, thisMachine],
  );
  // Another machine's latest backup first on a machine with none of its own.
  const first = choices.find((c) => c.set.machine_id !== thisMachine) ?? choices[0];
  const [pick, setPick] = useState(first?.id ?? "");
  const [list, setList] = useState<RestoreView | null>(null);
  const [skipped, setSkipped] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<RestoreReport | null>(null);
  const chosen = choices.find((c) => c.id === pick);

  const load = useCallback(() => {
    if (!chosen) return;
    backupRestoreList(chosen.set.folder, chosen.stamp).then(setList, (e) => onError(String(e)));
  }, [chosen, onError]);
  useEffect(() => {
    setList(null);
    setSkipped(new Set());
    load();
  }, [load]);

  if (choices.length === 0) {
    return <p className="text-[11px] text-dim">No backups in this folder yet, from this machine or any other.</p>;
  }

  const files = list?.files ?? [];
  const groups = groupByApp(files);
  const picked = files.filter((f) => !skipped.has(f.app));

  async function restore() {
    if (!chosen) return;
    setBusy(true);
    try {
      setResult(await restoreBackup(chosen.set.folder, chosen.stamp, picked.map(fileKey)));
      load();
    } catch (e) {
      onError(String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="flex flex-col gap-1 border-t border-line pt-1.5">
      <div className="flex items-center gap-1.5 text-[11px]">
        <span className="shrink-0 font-medium">Restore</span>
        <select
          aria-label="Backup to restore"
          className="h-6 min-w-0 flex-1 rounded-[5px] border border-line bg-panel px-1.5 text-[11px]"
          value={pick}
          onChange={(e) => {
            setPick(e.target.value);
            setResult(null);
          }}
        >
          {choices.map((c) => (
            <option key={c.id} value={c.id}>
              {c.label}
            </option>
          ))}
        </select>
        {picked.length > 0 && (
          <Button onClick={restore} disabled={busy} className="h-6 shrink-0 whitespace-nowrap px-2.5">
            {busy ? "Restoring…" : `Restore ${picked.length}`}
          </Button>
        )}
      </div>
      {list && files.length === 0 && !result && (
        <p className="text-[11px] text-ok">This machine already has everything in that backup.</p>
      )}
      {groups.length > 0 && (
        <ul className="flex flex-col">
          {groups.map(([app, fs]) => (
            <li key={app}>
              <label className="flex items-center gap-2 rounded-[5px] px-1 py-0.5 hover:bg-sel/50">
                <input
                  type="checkbox"
                  className="accent-accent"
                  checked={!skipped.has(app)}
                  onChange={() =>
                    setSkipped((s) => {
                      const n = new Set(s);
                      if (n.has(app)) n.delete(app);
                      else n.add(app);
                      return n;
                    })
                  }
                />
                <span className="w-24 shrink-0 font-medium">{app}</span>
                <span className="min-w-0 flex-1 truncate text-[11px] text-dim">
                  {app === "Installer Vault" ? "the list of installers" : summaryOf(fs)}
                  {replacing(fs) > 0 && `, ${replacing(fs)} replacing this machine's copy`}
                </span>
              </label>
            </li>
          ))}
        </ul>
      )}
      {result && <p className="text-[11px] text-ok">{restoreSummary(result)}</p>}
      {groups.length > 0 && !result && (
        <p className="text-[11px] text-dim">
          Each file goes to the right place for this machine, anything it replaces is saved first, and you can undo it in
          History. Plug-ins aren't in backups: install them from the Vault or their vendors.
        </p>
      )}
    </div>
  );
}

const replacing = (fs: FileToBring[]) => fs.filter((f) => f.replaces).length;

function restoreSummary(r: RestoreReport): string {
  let text =
    r.written > 0
      ? `Restored ${plural(r.written, "file")}. You can undo this in History.`
      : r.same > 0
        ? "This machine already had those files."
        : "Nothing was restored.";
  if (r.unavailable.length > 0)
    text += ` ${plural(r.unavailable.length, "file")} couldn't be read from the backup yet (still downloading?); try again shortly.`;
  if (r.held_back.length > 0)
    text += ` ${plural(r.held_back.length, "file")} waited for a file ${r.held_back.length === 1 ? "it needs" : "they need"}.`;
  return text;
}

/** "Dropbox", or the folder's name. */
function placeName(view: BackupView): string {
  const cloud = view.clouds.find((c) => c.path === view.settings.folder);
  if (cloud) return cloud.name;
  const f = view.settings.folder ?? "";
  return f.split(/[\\/]/).filter(Boolean).pop() ?? "your folder";
}

function when(unixSeconds: number): string {
  const s = Date.now() / 1000 - unixSeconds;
  if (s < 86400) return timeAgo(unixSeconds);
  return new Date(unixSeconds * 1000).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
}

function plural(n: number, one: string, many = `${one}s`): string {
  return `${n.toLocaleString()} ${n === 1 ? one : many}`;
}
