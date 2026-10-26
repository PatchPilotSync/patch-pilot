import { useCallback, useEffect, useMemo, useState, type ReactNode } from "react";
import {
  backUpNow,
  githubOpen,
  macKeysView,
  setMacKeys,
  type MacKeysView,
  backupRestoreList,
  backupView,
  fileKey,
  onBackupChanged,
  onGitHubProgress,
  onSyncChanged,
  restoreBackup,
  revealPath,
  SHOW_IN_FOLDER,
  setBackupAuto,
  setBackupFolder,
  setKeepInSync,
  setSyncKind,
  settleConflict,
  syncLookNow,
  syncPrefsView,
  syncView,
  type BackupSet,
  type BackupView,
  type FileToBring,
  type GitHubProgress,
  type RestoreReport,
  type RestoreView,
  type SkippedFile,
  type SyncConflict,
  type SyncKind,
  type SyncSource,
  type SyncView,
} from "./api";
import { FolderPicker } from "./BigFiles";
import { GitHubSetup } from "./GitHub";
import { groupByApp, summaryOf } from "./BringOver";
import { Button, Notice, timeAgo } from "./ui";
import { formatSize } from "./screens/VaultTriage";

/**
 * Sync and backup (decisions #67 and #70): one line on This machine. Once a
 * cloud folder is picked, new and changed templates, scripts and presets keep
 * themselves in sync with linked machines through it, and every change is
 * backed up there. Show opens the folder, the switches and Sync with. On a
 * fresh machine that finds another machine's backup, the line offers to
 * sync with it. `openAt` opens the line (the tray's Keep Files in Sync with no
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
  const progress = useGitHubProgress(view);
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
      ? "Keep your files in sync through a cloud folder, or sync with another machine's backup."
      : "Keep templates, scripts and presets in sync between your machines through a cloud folder.";
  } else if (!view.dir) {
    tone = "warn";
    text = "The cloud folder isn't there now (signed out, or not connected).";
  } else if (view.github && progress) {
    tone = "accent";
    text = progress.uploading
      ? `Uploading ${progress.done.toLocaleString()} of ${plural(progress.total, "file")} to GitHub…`
      : `Downloading ${progress.done.toLocaleString()} of ${plural(progress.total, "file")} from GitHub…`;
  } else if (view.github_error) {
    tone = "warn";
    text = view.github_error;
  } else if (view.error) {
    tone = "warn";
    text = `The last backup didn't finish: ${view.error}`;
  } else if (offerRestore) {
    tone = "accent";
    text = `A backup of ${others[0].machine_name} from ${when(others[0].backups[0].made_at)} is in ${where}. Sync with it?`;
  } else if (syncing) {
    [tone, text] = syncLine(sync, where, view.github);
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
          !offerRestore && (
            <>
              {view.github && (
                <Button
                  variant="quiet"
                  className="h-6 shrink-0 px-2"
                  title={`Open ${view.github} on GitHub`}
                  onClick={() => githubOpen("repo").catch((e) => onError(String(e)))}
                >
                  View on GitHub
                </Button>
              )}
              {progress ? null : syncing ? (
                // Looks on its own too; this is for "why hasn't it come over yet?".
                <Button variant="link" onClick={check} disabled={busy} className="h-6 shrink-0 px-2">
                  {busy ? "Checking…" : view.github_error ? "Try now" : "Check now"}
                </Button>
              ) : (
                // Backups run on their own while syncing; the button is for by-hand backups.
                <Button variant="link" onClick={now} disabled={busy || view.running} className="h-6 shrink-0 px-2">
                  {busy || view.running ? "Backing up…" : "Back up now"}
                </Button>
              )}
            </>
          )
        }
        open={open}
        onToggle={() => setOpen((o) => !o)}
        toggle={[view.settings.folder ? (offerRestore ? "Sync with…" : "Show") : "Set up", "Hide"]}
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
function syncLine(sync: SyncView, where: string, github: string | null): [Tone, string] {
  const last = sync.last;
  if (sync.linked === 0) return ["accent", `Backed up to ${where}. Link another machine to keep files in sync with it.`];
  if (last && last.waiting.length > 0) {
    // "Ableton Live is open": its files and settings change only while it's closed.
    const open = last.waiting.filter((w) => w.endsWith(" is open")).map((w) => w.slice(0, -" is open".length));
    const other = last.waiting.filter((w) => !w.endsWith(" is open"));
    if (other.length === 0)
      return ["warn", `Changes from your other machine are waiting for you to close ${open.join(" and ")}.`];
    return ["warn", `Some changes are waiting: ${last.waiting.join("; ")}. Trying again shortly.`];
  }
  const sources = (last?.sources ?? []).filter((s) => s.linked);
  if (!last) return ["ok", `In sync through ${where}. Checking your other machines' files…`];
  if (sources.length === 0)
    return github
      ? ["accent", `Next: on your other machine, Use GitHub… with ${github.split("/").pop()}.`]
      : ["accent", `No other machine's files in ${where} yet. Turn on Keep Files in Sync on your other machine too.`];
  const names = (f: (s: SyncSource) => boolean) => sources.filter(f).map((s) => s.machine_name).join(", ");
  const downloading = sources.reduce((a, s) => a + s.downloading, 0);
  if (downloading > 0)
    return ["accent", `Bringing over ${plural(downloading, "file")} from ${names((s) => s.downloading > 0)} as ${where} downloads them…`];
  // The last thing brought over stays in view for a while, even after later
  // looks found nothing new.
  const recent = sync.recent;
  if (recent && Date.now() / 1000 - recent.at < 30 * 60)
    return ["ok", `Synced ${recent.what} ${timeAgo(recent.at)}. Undo in History.`];
  return ["ok", `In sync with ${names(() => true)} through ${where}, checked ${timeAgo(last.looked_at)}.`];
}

/** How far a long GitHub sync has got, while one runs. */
function useGitHubProgress(view: BackupView | null) {
  const [progress, setProgress] = useState<GitHubProgress | null>(null);
  const initial = view?.github_progress ?? null;
  useEffect(() => setProgress(initial), [initial]);
  useEffect(() => {
    const unlisten = onGitHubProgress(setProgress);
    return () => {
      unlisten.then((f) => f());
    };
  }, []);
  return progress;
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

/** Set up's "Sync with a backup": pick the folder, then what to bring over. */
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
  const [advanced, setAdvanced] = useState(false);
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

  // Syncing is what GitHub is for: it goes on.
  async function usedGitHub() {
    setAdvanced(false);
    await act(
      backupView().then(async (v) => {
        if (sync && onSync && v.dir && !v.settings.keep_in_sync) {
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

  if (advanced) {
    return <GitHubSetup onUsed={usedGitHub} onCancel={() => setAdvanced(false)} onError={onError} />;
  }

  if (!view.dir || picking) {
    return (
      <div className="flex flex-col gap-1">
        <FolderPicker
          clouds={view.clouds}
          busy={busy}
          missing={view.settings.folder !== null && !view.dir}
          preferred={view.settings.folder ?? view.transfer_folder}
          onChoose={choose}
          onCancel={view.dir ? () => setPicking(false) : undefined}
          hint={sync ? <SyncHint /> : <BackupHint />}
        />
        <Button variant="link" className="-ml-3 h-6 self-start text-[11px]" onClick={() => setAdvanced(true)}>
          {view.github ? `Advanced: GitHub (${view.github})…` : "Advanced: sync with GitHub instead…"}
        </Button>
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
        {view.github ? (
          <span className="min-w-0 flex-1 truncate" title={`Private repo on GitHub, copied to ${view.dir}`}>
            GitHub repo {view.github}
          </span>
        ) : (
          <span className="min-w-48 flex-1 truncate" title={view.dir}>
            {view.dir}
          </span>
        )}
        <Button variant="quiet" className="h-6 px-2" onClick={() => setPicking(true)}>
          {view.github ? "Change" : "Change folder"}
        </Button>
        {/* Advanced (decision #73): in view once a folder is picked, not only behind Change folder. */}
        {!view.github && (
          <Button variant="quiet" className="h-6 px-2" title="Advanced: sync through a private GitHub repo instead" onClick={() => setAdvanced(true)}>
            Use GitHub…
          </Button>
        )}
      </div>
      {last && (
        <p className="text-[11px] text-dim">
          {last.unchanged
            ? `This machine's backup is current (${plural(last.files, "file")}).`
            : `This machine's last backup copied ${plural(last.copied, "changed file")}${last.versioned ? ` and kept ${plural(last.versioned, "earlier copy", "earlier copies")}` : ""}.`}
        </p>
      )}
      {last && last.skipped.length > 0 && <Skipped files={last.skipped} onError={onError} />}
      {sync?.on && <SyncSummary sync={sync} />}
      {sync?.on && <SyncChoices onError={onError} />}
      {/* While syncing, this machine's own backups would only offer its own files back. */}
      <Restore
        sets={view.sets}
        thisMachine={view.this_machine}
        othersOnly={view.settings.keep_in_sync}
        onError={onError}
      />
    </div>
  );
}

/** "2 files left out", opening to say which, why, and what to do. */
function Skipped({ files, onError }: { files: SkippedFile[]; onError: (e: string) => void }) {
  const [open, setOpen] = useState(false);
  return (
    <div className="text-[11px]">
      <Button variant="link" className="h-5 px-0 text-warn" onClick={() => setOpen(!open)}>
        {plural(files.length, "file")} left out of the backup {open ? "▾" : "▸"}
      </Button>
      {open && (
        <ul className="mt-1 space-y-1.5">
          {files.map((f) => (
            <li key={f.path} className="min-w-0">
              <div className="truncate font-medium" title={f.path}>
                {f.name}
              </div>
              <div className="flex min-w-0 items-center gap-2">
                <span className="min-w-0 truncate text-dim" title={f.path}>
                  {f.path}
                </span>
                <Button
                  variant="link"
                  className="h-5 shrink-0 px-0"
                  onClick={() => revealPath(f.path).catch((e) => onError(String(e)))}
                >
                  {SHOW_IN_FOLDER}
                </Button>
              </div>
              <div className="text-dim">
                {f.too_big !== null
                  ? `Too big to back up (${formatSize(f.too_big)}), so it stays on this machine only. Copy it over yourself if another machine needs it.`
                  : `Couldn't be read: ${f.error ?? "unknown error"}. Close any app that has it open, then Back up now.`}
              </div>
            </li>
          ))}
        </ul>
      )}
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
  const missing = sync.last?.missing_places ?? [];
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
                s.renamed > 0 && `${s.renamed.toLocaleString()} renamed`,
                s.to_choose > 0 && `${s.to_choose.toLocaleString()} to choose`,
                s.downloading > 0 && `${s.downloading.toLocaleString()} downloading`,
              ]
                .filter(Boolean)
                .join(", ")
            : "not linked to this machine, so left alone"}
        </p>
      ))}
      {missing.length > 0 && (
        <p className="text-warn">
          {missing.length === 1 ? "Live Place" : "Live Places"} whose folder isn't on this machine yet: {missing.join(", ")}.
        </p>
      )}
      <p>
        When two machines first sync, the Primary Rig's copy of a file wins. After that, new, changed and renamed files
        travel both ways, and so do Reaper's shortcuts, menus and mouse modifiers and Live's Collection names and Places
        in Dropbox or another cloud folder (once the app is closed). Deleted files and machine settings (audio device,
        MIDI, plug-in folders) never sync.
      </p>
      {sync.error && <p className="text-warn">{sync.error}</p>}
    </div>
  );
}

/**
 * Sync preferences (decision #80): a checkbox per kind of file or setting,
 * under Reaper, Ableton Live and Plug-ins. A kind turned off stays this
 * machine's own: it neither comes here nor goes from here.
 */
function SyncChoices({ onError }: { onError: (e: string) => void }) {
  const [open, setOpen] = useState(false);
  const [kinds, setKinds] = useState<SyncKind[] | null>(null);
  useEffect(() => {
    syncPrefsView().then(setKinds, (e) => onError(String(e)));
  }, [onError]);
  if (!kinds) return null;
  const off = kinds.filter((k) => !k.on).length;
  const apps = [...new Set(kinds.map((k) => k.app))];

  async function toggle(id: string, on: boolean) {
    setKinds((ks) => ks && ks.map((k) => (k.id === id ? { ...k, on } : k)));
    try {
      setKinds(await setSyncKind(id, on));
    } catch (e) {
      onError(String(e));
    }
  }

  return (
    <div className="text-[11px]">
      <Button variant="link" className="h-5 px-0" onClick={() => setOpen(!open)}>
        Choose what syncs{off > 0 ? ` (${off} kept on this machine only)` : ""} {open ? "▾" : "▸"}
      </Button>
      {open && (
        <div className="mt-1 flex flex-col gap-2">
          {apps.map((app) => (
            <section key={app}>
              <h4 className="mb-0.5 font-medium text-fg">{app}</h4>
              <div className="grid grid-cols-1 gap-x-4 sm:grid-cols-2">
                {kinds
                  .filter((k) => k.app === app)
                  .map((k) => (
                    <label key={k.id} className="flex items-center gap-1.5 text-dim">
                      <input
                        type="checkbox"
                        className="accent-accent"
                        checked={k.on}
                        onChange={(e) => toggle(k.id, e.target.checked)}
                      />
                      {k.label}
                    </label>
                  ))}
              </div>
              {app === "Reaper" && <MacShortcuts onError={onError} />}
            </section>
          ))}
          <p className="text-dim">
            Unticked kinds stay this machine's own: changes to them don't come here or go from here. Backups and Sync
            with still have them.
          </p>
        </div>
      )}
    </div>
  );
}

/**
 * Reaper shortcuts on a Mac (decision #81). Reaper already reads a PC's Ctrl,
 * Alt and Win as Cmd, Option and Control; this setting moves the few Cmd
 * shortcuts macOS keeps for itself to Control. On a PC it only explains.
 */
function MacShortcuts({ onError }: { onError: (e: string) => void }) {
  const [view, setView] = useState<MacKeysView | null>(null);
  useEffect(() => {
    macKeysView().then(setView, (e) => onError(String(e)));
  }, [onError]);
  if (!view) return null;
  const { moved, missing } = view.shortcuts;

  async function toggle(on: boolean) {
    setView((v) => v && { ...v, on });
    try {
      setView(await setMacKeys(on));
    } catch (e) {
      onError(String(e));
    }
  }

  const list = moved.map(([from, to, n]) => `${from} → ${to}${n > 1 ? ` (${n})` : ""}`).join(", ");
  return (
    <div className="mt-1.5 flex flex-col gap-0.5 text-dim">
      {view.here ? (
        <label className="flex items-center gap-1.5">
          <input
            type="checkbox"
            className="accent-accent"
            checked={view.on}
            onChange={(e) => toggle(e.target.checked)}
          />
          Move shortcuts macOS keeps for itself to Control
        </label>
      ) : (
        <p className="font-medium text-fg">Shortcuts on a Mac</p>
      )}
      <p>
        Reaper on a Mac reads a PC's Ctrl as Cmd, Alt as Option and Win as Control, so most shortcuts work as they are.
        A few Cmd shortcuts belong to macOS (Cmd+H hides Reaper, Cmd+Space opens Spotlight, Cmd+Shift+3 takes a
        screenshot), so {view.here ? "they move" : "each Mac moves them"} to the same key with Control, and back again
        when they go to a PC.
      </p>
      {moved.length > 0 && (
        <p>
          {view.here ? (view.on ? "Moved here: " : "Would move: ") : "In this key map: "}
          {list}.
        </p>
      )}
      {view.waiting && <p className="text-warn">Waiting for Reaper to close to change its shortcuts.</p>}
      {missing.length > 0 && (
        <p>
          {missing.map(([key, n]) => `${n} on ${key}`).join(", ")}: {missing.length === 1 ? "a key" : "keys"} Mac
          keyboards don't have. Give {missing.reduce((a, [, n]) => a + n, 0) === 1 ? "it" : "them"} another key in
          Reaper's Actions list.
        </p>
      )}
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
  othersOnly = false,
  onError,
}: {
  sets: BackupSet[];
  thisMachine: string | null;
  /** Leave this machine's own backups out; nothing shows when there are no others. */
  othersOnly?: boolean;
  onError: (e: string) => void;
}) {
  // A machine's latest backup is picked by "latest", not its stamp, so the
  // pick moves on to each newer backup as it arrives. While syncing only the
  // latest is offered: an older one would bring back what has since changed.
  const choices = useMemo(
    () =>
      sets
        .filter((s) => !othersOnly || s.machine_id !== thisMachine)
        .flatMap((s) =>
          s.backups.slice(0, othersOnly ? 1 : undefined).map((b, i) => ({
            id: `${s.folder}\n${i === 0 ? "latest" : b.stamp}`,
            set: s,
            stamp: b.stamp,
            label: `${s.machine_id === thisMachine ? "This machine" : s.machine_name} · ${when(b.made_at)}${i === 0 ? " (latest)" : ""} · ${plural(b.files, "file")}`,
          })),
        ),
    [sets, thisMachine, othersOnly],
  );
  // Another machine's latest backup first on a machine with none of its own.
  const first = choices.find((c) => c.set.machine_id !== thisMachine) ?? choices[0];
  const [pick, setPick] = useState(first?.id ?? "");
  const [list, setList] = useState<RestoreView | null>(null);
  const [skipped, setSkipped] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<RestoreReport | null>(null);
  // Why the chosen backup can't be listed (still downloading, gone, damaged);
  // shown under it rather than as an error across the window.
  const [unreadable, setUnreadable] = useState<string | null>(null);
  // A backup that's gone (or a list that has just appeared) falls back to the default.
  const chosen = choices.find((c) => c.id === pick) ?? first;
  const folder = chosen?.set.folder;
  const stamp = chosen?.stamp;

  // Keyed on the backup, not `chosen`: the list of backups reloads with every
  // sync, and a 15 MB backup shouldn't be read again each time.
  const load = useCallback(() => {
    if (folder === undefined || stamp === undefined) return;
    backupRestoreList(folder, stamp).then(
      (l) => {
        setUnreadable(null);
        setList(l);
      },
      (e) => setUnreadable(String(e)),
    );
  }, [folder, stamp]);
  useEffect(() => {
    setList(null);
    setUnreadable(null);
    setSkipped(new Set());
    load();
  }, [load]);

  if (choices.length === 0) {
    if (othersOnly) return null;
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
        <span className="shrink-0 font-medium">Sync with</span>
        <select
          aria-label="Backup to sync with"
          className="h-6 min-w-0 flex-1 rounded-[5px] border border-line bg-panel px-1.5 text-[11px]"
          value={chosen?.id ?? ""}
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
            {busy ? "Syncing…" : `Sync ${plural(picked.length, "file")}`}
          </Button>
        )}
      </div>
      {unreadable && (
        <p className="text-[11px] text-warn">
          {unreadable}{" "}
          <Button variant="link" onClick={load} className="h-auto p-0 text-[11px]">
            Try again
          </Button>
        </p>
      )}
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
      ? `Brought over ${plural(r.written, "file")}. You can undo this in History.`
      : r.same > 0
        ? "This machine already had those files."
        : "Nothing was brought over.";
  if (r.unavailable.length > 0)
    text += ` ${plural(r.unavailable.length, "file")} couldn't be read from the backup yet (still downloading?); try again shortly.`;
  if (r.held_back.length > 0)
    text += ` ${plural(r.held_back.length, "file")} waited for a file ${r.held_back.length === 1 ? "it needs" : "they need"}.`;
  return text;
}

/** "Dropbox", or the folder's name. */
function placeName(view: BackupView): string {
  if (view.github) return "GitHub";
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
