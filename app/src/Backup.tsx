import { useCallback, useEffect, useMemo, useState, type ReactNode } from "react";
import {
  backUpNow,
  backupRestoreList,
  backupView,
  fileKey,
  onBackupChanged,
  restoreBackup,
  setBackupAuto,
  setBackupFolder,
  type BackupSet,
  type BackupView,
  type FileToBring,
  type RestoreReport,
  type RestoreView,
} from "./api";
import { FolderPicker } from "./BigFiles";
import { groupByApp, summaryOf } from "./BringOver";
import { Button, Notice, timeAgo } from "./ui";

/**
 * Back up everything (decision #67): one line on This machine saying when it
 * last backed up and where, with Back up now; Show opens the folder, the
 * automatic switch and Restore. On a fresh machine that finds another
 * machine's backup, the line offers to restore it.
 */
export function BackupLine({ onError, fresh }: { onError: (e: string) => void; fresh: boolean }) {
  const [view, setView, load] = useBackupView(onError);
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);

  if (!view) return null;

  const mine = view.sets.find((s) => s.machine_id === view.this_machine);
  const others = view.sets.filter((s) => s.machine_id !== view.this_machine && s.backups.length > 0);
  const where = placeName(view);
  const latest = mine?.backups[0];

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

  let tone: "ok" | "accent" | "warn" = "ok";
  let text: ReactNode;
  if (!view.settings.folder) {
    tone = "accent";
    text = fresh
      ? "Back up this machine to a cloud folder, or restore one from a backup."
      : "Back up your templates, scripts, presets and settings to a cloud folder.";
  } else if (!view.dir) {
    tone = "warn";
    text = "The backup folder isn't there now (signed out, or not connected).";
  } else if (view.error) {
    tone = "warn";
    text = `The last backup didn't finish: ${view.error}`;
  } else if (fresh && !latest && others.length > 0) {
    tone = "accent";
    text = `A backup of ${others[0].machine_name} from ${when(others[0].backups[0].made_at)} is in ${where}. Restore it here?`;
  } else if (latest) {
    text = `Backed up ${timeAgo(latest.made_at)} to ${where}${view.settings.auto ? ", after every change" : ""}.`;
  } else {
    tone = "accent";
    text = `Not backed up yet. Backups go to ${where}.`;
  }

  const offerRestore = fresh && others.length > 0 && !latest;
  return (
    <Notice
      tone={tone}
      text={
        <span className="text-dim" title={view.dir ?? undefined}>
          <span className="font-medium text-fg">Backup</span> · {text}
        </span>
      }
      actions={
        view.dir &&
        !(fresh && !latest && others.length > 0) && (
          <Button variant="link" onClick={now} disabled={busy || view.running} className="h-6 shrink-0 px-2">
            {busy || view.running ? "Backing up…" : "Back up now"}
          </Button>
        )
      }
      open={open}
      onToggle={() => setOpen((o) => !o)}
      toggle={[view.settings.folder ? (offerRestore ? "Restore…" : "Show") : "Set up", "Hide"]}
    >
      <Details view={view} onView={setView} onError={onError} />
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
  onError,
}: {
  view: BackupView;
  onView: (v: BackupView) => void;
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

  if (!view.dir || picking) {
    return (
      <div>
        <FolderPicker
          clouds={view.clouds}
          busy={busy}
          missing={view.settings.folder !== null && !view.dir}
          preferred={view.settings.folder ?? view.transfer_folder}
          onChoose={(service, folder) => act(setBackupFolder(service, folder))}
          onCancel={view.dir ? () => setPicking(false) : undefined}
          hint={
            <>
              Pick where backups go. Patch Pilot keeps them in a <span className="font-medium">Patch Pilot Backup</span>{" "}
              folder there, one folder per machine, and never deletes them.
            </>
          }
        />
      </div>
    );
  }

  const last = view.last;
  return (
    <div className="flex flex-col gap-1.5">
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-[11px] text-dim">
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
      <Restore sets={view.sets} thisMachine={view.this_machine} onError={onError} />
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
