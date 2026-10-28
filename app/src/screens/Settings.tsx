import { useEffect, useState, type ReactNode } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { emit } from "@tauri-apps/api/event";
import {
  backupView,
  onBackupChanged,
  onSettingsChanged,
  onUiChanged,
  setDriftAlerts,
  setOpenAtLogin,
  setTextSize,
  setTheme,
  settingsView,
  type MachineCard,
  type SettingsView,
  type SyncStatus,
  type Theme,
} from "../api";
import { BackupLine, SyncChoices } from "../Backup";
import { IgnoredToggle, useIgnores } from "../Ignore";
import { Button, Label, Loading } from "../ui";

/**
 * Settings (decision #96): one place for what's otherwise spread over This
 * machine and the tray menu. Each section is the same control as elsewhere
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
  }, [onError]);

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
      </Section>

      <Section title="Startup and alerts">
        <Check
          checked={view.open_at_login}
          onChange={(on) => save(setOpenAtLogin(on), { open_at_login: on })}
          label="Open at login, in the tray"
          hint="Patch Pilot watches for changes quietly, without a window."
        />
        <Check
          checked={view.drift_alerts}
          onChange={(on) => save(setDriftAlerts(on), { drift_alerts: on })}
          label="Drift alerts"
          hint="A notification when a machine falls further behind the Primary Rig."
        />
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
      <span className="w-20 shrink-0 text-dim">{label}</span>
      {children}
    </div>
  );
}

function Check({
  checked,
  onChange,
  label,
  hint,
}: {
  checked: boolean;
  onChange: (on: boolean) => void;
  label: string;
  hint: string;
}) {
  return (
    <label className="flex min-h-7 items-center gap-2 text-xs">
      <input type="checkbox" className="accent-accent" checked={checked} onChange={(e) => onChange(e.target.checked)} />
      <span>{label}</span>
      <span className="min-w-0 truncate text-[11px] text-dim">{hint}</span>
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
    <div role="radiogroup" className="inline-flex rounded-md border border-line bg-panel p-0.5">
      {options.map(([v, label]) => (
        <button
          key={String(v)}
          role="radio"
          aria-checked={v === value}
          onClick={() => v !== value && onChange(v)}
          className={`h-6 rounded-[5px] px-2.5 text-xs transition-colors ${
            v === value ? "bg-canvas font-medium text-fg shadow-sm" : "text-dim hover:text-fg"
          }`}
        >
          {label}
        </button>
      ))}
    </div>
  );
}

const SIZE_NAMES: Record<string, string> = { "0.9": "Smaller", "1": "Default", "1.1": "Larger", "1.2": "Largest" };

function sizeName(size: number): string {
  return SIZE_NAMES[String(size)] ?? `${Math.round(size * 100)}%`;
}
