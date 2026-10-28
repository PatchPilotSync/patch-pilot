import { useEffect, useState, type ReactNode } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { emit } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { relaunch } from "@tauri-apps/plugin-process";
import {
  backupView,
  onBackupChanged,
  onSettingsChanged,
  onUiChanged,
  openDataFolder,
  resetThisMachine,
  setAppPrefs,
  setCompact,
  setDawWatched,
  setDriftAlerts,
  setOpenAtLogin,
  setTextSize,
  setTheme,
  settingsView,
  SHOW_IN_FOLDER,
  type AppPrefs,
  type MachineCard,
  type PluginFormat,
  type SettingsView,
  type SyncStatus,
  type Theme,
} from "../api";
import { BackupLine, SyncChoices } from "../Backup";
import { IgnoredToggle, useIgnores } from "../Ignore";
import { Button, Label, Loading } from "../ui";

/**
 * Settings (decisions #96 and #97): one place for what's otherwise spread
 * over This machine and the tray menu, plus how this machine rescans, warns
 * and keeps its data. Each section that exists elsewhere is the same control
 * (Sync & backup's line, Choose what syncs, the ignore list), not a copy, so
 * a change shows in both places. Opened from the top bar or the tray menu.
 */
export function Settings({
  machines,
  link,
  refreshKey,
  onLink,
  onError,
}: {
  machines: MachineCard[];
  link: SyncStatus | null;
  refreshKey: number;
  onLink: () => void;
  onError: (e: string) => void;
}) {
  const [view, setView] = useState<SettingsView | null>(null);
  const [syncing, setSyncing] = useState<boolean | null>(null);
  const [version, setVersion] = useState("");
  const ignores = useIgnores(null, refreshKey);

  useEffect(() => {
    const load = () => settingsView().then(setView, (e) => onError(String(e)));
    const loadSync = () => backupView().then((v) => setSyncing(v.settings.keep_in_sync), () => {});
    load();
    loadSync();
    getVersion().then(setVersion, () => {});
    const offTray = onSettingsChanged(load);
    const offUi = onUiChanged(setView);
    const offBackup = onBackupChanged(loadSync);
    return () => {
      offTray.then((f) => f());
      offUi.then((f) => f());
      offBackup.then((f) => f());
    };
  }, [onError, refreshKey]);

  if (!view) return <Loading text="Loading settings…" compact />;

  async function save(p: Promise<SettingsView>, optimistic: Partial<SettingsView>) {
    setView((v) => v && { ...v, ...optimistic });
    try {
      setView(await p);
    } catch (e) {
      onError(String(e));
      settingsView().then(setView, () => {});
    }
  }
  const prefs = view.prefs;
  const savePrefs = (change: Partial<AppPrefs>) => {
    const next = { ...prefs, ...change };
    save(setAppPrefs(next), { prefs: next });
  };
  const quiet = prefs.quiet_hours;

  const others = machines.filter((m) => !m.is_this_machine);

  return (
    <div className="flex flex-col pb-6">
      <header className="border-b border-line px-4 py-2.5">
        <h1 className="text-sm font-semibold">Settings</h1>
        <p className="text-xs text-dim">For this machine. Each machine keeps its own.</p>
      </header>

      <Section title="Sync">
        <div className="-mx-2">
          <BackupLine onError={onError} fresh={false} choices={false} />
        </div>
      </Section>

      <Section title="What syncs">
        {syncing === false && (
          <p className="mb-1.5 text-[11px] text-dim">
            Keeping files in sync is off on this machine, so nothing syncs yet. These choices apply once it's on.
          </p>
        )}
        <SyncChoices onError={onError} listed />
      </Section>

      <Section title="Look">
        <Row label="Text size">
          <Segmented
            value={view.text_size}
            options={view.text_sizes.map((s) => [s, sizeName(s)])}
            onChange={(s) => save(setTextSize(s), { text_size: s })}
          />
        </Row>
        <Row label="Theme">
          <Segmented<Theme>
            value={view.theme}
            options={[
              ["system", "Match system"],
              ["light", "Light"],
              ["dark", "Dark"],
            ]}
            onChange={(t) => save(setTheme(t), { theme: t })}
          />
        </Row>
        <Row label="Layout">
          <Segmented
            value={view.compact ? "compact" : "comfortable"}
            options={[
              ["comfortable", "Comfortable"],
              ["compact", "Compact"],
            ]}
            onChange={(d) => save(setCompact(d === "compact"), { compact: d === "compact" })}
          />
        </Row>
      </Section>

      <Section title="Scanning">
        <Row label="Rescan">
          <Segmented<AppPrefs["rescan"]>
            value={prefs.rescan}
            options={[
              ["on_changes", "When folders change"],
              ["daily", "Also once a day"],
              ["manual", "Only when I press Rescan"],
            ]}
            onChange={(rescan) => savePrefs({ rescan })}
          />
        </Row>
        <Check
          checked={prefs.rescan_after_update}
          onChange={(on) => savePrefs({ rescan_after_update: on })}
          label="Rescan right after an update"
          hint="So This machine shows what the new version reads."
        />
        <Row label="DAWs here">
          <Check
            checked={view.reaper}
            onChange={(on) => save(setDawWatched("reaper", on), { reaper: on })}
            label="Reaper"
          />
          <Check
            checked={view.live}
            onChange={(on) => save(setDawWatched("live", on), { live: on })}
            label="Ableton Live"
          />
          <span className="min-w-0 truncate text-[11px] text-dim">
            Untick one this machine doesn't use: its files stop counting here.
          </span>
        </Row>
        <ExtraFolders
          folders={prefs.extra_folders}
          onChange={(extra_folders) => savePrefs({ extra_folders })}
          onError={onError}
        />
      </Section>

      <Section title="Startup">
        <Check
          checked={view.open_at_login}
          onChange={(on) => save(setOpenAtLogin(on), { open_at_login: on })}
          label="Open at login, in the tray"
          hint="Patch Pilot watches for changes quietly."
        />
        <Check
          checked={prefs.window_at_login}
          disabled={!view.open_at_login}
          onChange={(on) => savePrefs({ window_at_login: on })}
          label="Show the window too when it opens at login"
        />
      </Section>

      <Section title="Notifications">
        <Check
          checked={view.drift_alerts}
          onChange={(on) => save(setDriftAlerts(on), { drift_alerts: on })}
          label="Drift alerts"
          hint="When a machine falls further behind the Primary Rig."
        />
        <Check
          checked={prefs.sync_notices}
          onChange={(on) => savePrefs({ sync_notices: on })}
          label="Files kept in sync"
          hint="When files arrive from another machine, or changed on both."
        />
        <Check
          checked={prefs.update_notices}
          onChange={(on) => savePrefs({ update_notices: on })}
          label="Offer updates"
          hint="Off: only Check for Updates looks."
        />
        <div className="flex min-h-7 items-center gap-2 text-xs">
          <label className="flex items-center gap-2">
            <input
              type="checkbox"
              className="accent-accent"
              checked={!!quiet}
              onChange={(e) => savePrefs({ quiet_hours: e.target.checked ? { from: 22, to: 8 } : null })}
            />
            Quiet hours
          </label>
          {quiet && (
            <>
              <HourPicker value={quiet.from} label="Quiet from" onChange={(from) => savePrefs({ quiet_hours: { ...quiet, from } })} />
              <span className="text-dim">to</span>
              <HourPicker value={quiet.to} label="Quiet until" onChange={(to) => savePrefs({ quiet_hours: { ...quiet, to } })} />
            </>
          )}
          <span className="min-w-0 truncate text-[11px] text-dim">No notifications then; the app still shows everything.</span>
        </div>
      </Section>

      <Section title="Machines">
        <div className="flex items-center gap-2 text-xs">
          <span className="min-w-0 flex-1 truncate text-dim">
            {!link?.linked
              ? "Not linked to another machine yet."
              : `Linked${link.online ? "" : " (offline right now)"}${
                  others.length ? `: ${others.map((m) => m.name).join(", ")}` : ""
                }.`}
          </span>
          <Button variant="link" className="h-6 shrink-0 px-2" onClick={onLink}>
            {link?.linked ? "Link another or unlink…" : "Link a machine…"}
          </Button>
        </div>
      </Section>

      <Section title="Hidden items">
        {ignores.length ? (
          <IgnoredToggle ignores={ignores} machine={null} onError={onError} listed />
        ) : (
          <p className="text-[11px] text-dim">
            Nothing hidden. Ignore on any line in Compare or Get Up To Speed hides it, and it shows up here to bring
            back.
          </p>
        )}
      </Section>

      <Section title="Data">
        <Row label="Undo history">
          <select
            aria-label="Keep restore points for"
            value={prefs.restore_days ?? 0}
            onChange={(e) => savePrefs({ restore_days: Number(e.target.value) || null })}
            className="h-6 rounded-md border border-line bg-canvas px-1 text-xs"
          >
            <option value={0}>Keep everything</option>
            <option value={365}>Keep a year</option>
            <option value={90}>Keep 90 days</option>
            <option value={30}>Keep 30 days</option>
          </select>
          <span className="min-w-0 truncate text-[11px] text-dim">
            Older changes leave History and their saved copies are freed.
          </span>
        </Row>
        <DataFolder dataDir={view.data_dir} onError={onError} />
      </Section>

      <Section title="About">
        <div className="flex items-center gap-2 text-xs">
          <span className="flex-1 text-dim">Patch Pilot {version}</span>
          <Button
            variant="link"
            className="h-6 shrink-0 px-2"
            onClick={() => emit("check-for-updates").catch((e) => onError(String(e)))}
          >
            Check for Updates
          </Button>
        </div>
      </Section>
    </div>
  );
}

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="border-b border-line px-4 py-2.5">
      <Label className="mb-1.5 font-medium">{title}</Label>
      {children}
    </section>
  );
}

function Row({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="flex min-h-8 items-center gap-3 text-xs">
      <span className="w-24 shrink-0 text-dim">{label}</span>
      {children}
    </div>
  );
}

function Check({
  checked,
  onChange,
  label,
  hint,
  disabled,
}: {
  checked: boolean;
  onChange: (on: boolean) => void;
  label: string;
  hint?: string;
  disabled?: boolean;
}) {
  return (
    <label className={`flex min-h-7 shrink-0 items-center gap-2 text-xs ${disabled ? "opacity-50" : ""}`}>
      <input
        type="checkbox"
        className="accent-accent"
        checked={checked}
        disabled={disabled}
        onChange={(e) => onChange(e.target.checked)}
      />
      <span>{label}</span>
      {hint && <span className="min-w-0 truncate text-[11px] text-dim">{hint}</span>}
    </label>
  );
}

/** A row of small buttons, one picked. */
function Segmented<T extends string | number>({
  value,
  options,
  onChange,
}: {
  value: T;
  options: [T, string][];
  onChange: (v: T) => void;
}) {
  return (
    <div role="radiogroup" className="inline-flex shrink-0 rounded-md border border-line bg-panel p-0.5">
      {options.map(([v, label]) => (
        <button
          key={String(v)}
          role="radio"
          aria-checked={v === value}
          onClick={() => v !== value && onChange(v)}
          className={`h-6 rounded-[5px] px-2.5 text-xs whitespace-nowrap transition-colors ${
            v === value ? "bg-canvas font-medium text-fg shadow-sm" : "text-dim hover:text-fg"
          }`}
        >
          {label}
        </button>
      ))}
    </div>
  );
}

function HourPicker({ value, label, onChange }: { value: number; label: string; onChange: (h: number) => void }) {
  return (
    <select
      aria-label={label}
      value={value}
      onChange={(e) => onChange(Number(e.target.value))}
      className="h-6 rounded-md border border-line bg-canvas px-1 text-xs"
    >
      {Array.from({ length: 24 }, (_, h) => (
        <option key={h} value={h}>
          {new Date(2000, 0, 1, h).toLocaleTimeString([], { hour: "numeric" })}
        </option>
      ))}
    </select>
  );
}

const FORMATS: [PluginFormat, string][] = [
  ["vst3", "VST3"],
  ["vst2", "VST2"],
  ["clap", "CLAP"],
  ["au", "AU"],
  ["aax", "AAX"],
];

/** Plug-in folders to scan besides the standard ones, e.g. on a second drive. */
function ExtraFolders({
  folders,
  onChange,
  onError,
}: {
  folders: AppPrefs["extra_folders"];
  onChange: (f: AppPrefs["extra_folders"]) => void;
  onError: (e: string) => void;
}) {
  const [format, setFormat] = useState<PluginFormat>("vst3");
  async function add() {
    try {
      const path = await open({ directory: true, title: `Pick a ${format.toUpperCase()} plug-in folder` });
      if (typeof path === "string" && !folders.some((f) => f.path === path && f.format === format)) {
        onChange([...folders, { format, path }]);
      }
    } catch (e) {
      onError(String(e));
    }
  }
  return (
    <div className="text-xs">
      <div className="flex min-h-8 items-center gap-3">
        <span className="w-24 shrink-0 text-dim">More plug-ins</span>
        <select
          aria-label="Plug-in format"
          value={format}
          onChange={(e) => setFormat(e.target.value as PluginFormat)}
          className="h-6 rounded-md border border-line bg-canvas px-1 text-xs"
        >
          {FORMATS.map(([f, name]) => (
            <option key={f} value={f}>
              {name}
            </option>
          ))}
        </select>
        <Button variant="link" className="-ml-2 h-6 px-2" onClick={add}>
          Add folder…
        </Button>
        <span className="min-w-0 truncate text-[11px] text-dim">
          {folders.length ? "Scanned with the standard folders." : "For plug-ins outside the standard folders."}
        </span>
      </div>
      {folders.map((f) => (
        <div key={`${f.format}:${f.path}`} className="flex h-7 items-center gap-3 rounded-[5px] hover:bg-sel/50">
          <span className="w-24 shrink-0 text-right text-[11px] text-dim">{f.format.toUpperCase()}</span>
          <span className="min-w-0 flex-1 truncate" title={f.path}>
            {f.path}
          </span>
          <Button variant="quiet" className="h-6 px-2" onClick={() => onChange(folders.filter((x) => x !== f))}>
            Remove
          </Button>
        </div>
      ))}
    </div>
  );
}

/** The data folder, and starting over on this machine after a confirm. */
function DataFolder({ dataDir, onError }: { dataDir: string; onError: (e: string) => void }) {
  const [asking, setAsking] = useState(false);
  const [busy, setBusy] = useState(false);
  async function startOver() {
    setBusy(true);
    try {
      await resetThisMachine();
      await relaunch();
    } catch (e) {
      onError(String(e));
      setBusy(false);
    }
  }
  return (
    <div className="text-xs">
      <div className="flex min-h-8 items-center gap-3">
        <span className="w-24 shrink-0 text-dim">Data folder</span>
        <span className="min-w-0 flex-1 truncate text-dim" title={dataDir}>
          {dataDir}
        </span>
        <Button variant="quiet" className="h-6 shrink-0 px-2" onClick={() => openDataFolder().catch((e) => onError(String(e)))}>
          {SHOW_IN_FOLDER}
        </Button>
        <Button variant="quiet" className="h-6 shrink-0 px-2" onClick={() => setAsking(true)} disabled={asking}>
          Start over…
        </Button>
      </div>
      {asking && (
        <div className="mt-1 flex items-center gap-2 rounded-md bg-warn/10 px-2 py-1.5">
          <p className="flex-1">
            Patch Pilot forgets this machine's scans, links and settings and restarts as on a new install. Plug-ins,
            DAW files and backups stay as they are, and the old data is kept in the data folder.
          </p>
          <Button variant="quiet" className="h-6 shrink-0 px-2" onClick={startOver} disabled={busy}>
            {busy ? "Restarting…" : "Start over"}
          </Button>
          <Button variant="quiet" className="h-6 shrink-0 px-2" onClick={() => setAsking(false)} disabled={busy}>
            Cancel
          </Button>
        </div>
      )}
    </div>
  );
}

const SIZE_NAMES: Record<string, string> = { "0.9": "Smaller", "1": "Default", "1.1": "Larger", "1.2": "Largest" };

function sizeName(size: number): string {
  return SIZE_NAMES[String(size)] ?? `${Math.round(size * 100)}%`;
}
