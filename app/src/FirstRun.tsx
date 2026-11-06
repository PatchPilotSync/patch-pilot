import { useEffect, useMemo, useState, type ReactNode } from "react";
import {
  backupView,
  lanUse,
  makeGolden,
  setBackupFolder,
  setDawWatched,
  setKeepInSync,
  setSyncKind,
  syncPrefsView,
  type BackupView,
  type Overview,
  type SyncKind,
} from "./api";
import { FolderPicker } from "./BigFiles";
import { GitHubSetup } from "./GitHub";
import { ChoiceRow, SYNC_METHODS, type SyncMethod } from "./SyncMethods";
import { AppMark } from "./screens/Welcome";
import { markTourSeen } from "./Tour";
import { Button, inputClass, Notice, Panel } from "./ui";

/**
 * The first-run questions (decision #121): a few short steps before the first
 * scan that set the app up the way this person works, instead of a list of
 * settings to find later. Each answer changes a real setting:
 * - main machine or not: the first scan becomes the Primary Rig, or the Link
 *   dialog opens to join the main machine;
 * - DAWs: an unticked DAW stops being watched here, and its files stop syncing;
 * - how files travel: the cloud folder or GitHub repo is picked right here and
 *   keeping files in sync goes on; the home network goes on once linked;
 * - what to keep the same: Choose what syncs, in a few plain groups.
 * Everything stays changeable in Settings.
 */

export type Role = "main" | "other" | "later";

export interface FirstRunAnswers {
  name: string;
  role: Role;
  reaper: boolean;
  live: boolean;
  method: SyncMethod | "later";
}

const STEPS = ["machine", "daws", "method", "what", "ready"] as const;
type StepId = (typeof STEPS)[number];

const LAN_KEY = "patchpilot.firstrun.lan";

export function FirstRun({
  onScan,
  onTour,
  onError,
}: {
  onScan: (answers: FirstRunAnswers) => void;
  onTour: () => void;
  onError: (e: string) => void;
}) {
  const [step, setStep] = useState<StepId>("machine");
  const [answers, setAnswers] = useState<FirstRunAnswers>({
    name: "",
    role: "main",
    reaper: true,
    live: true,
    method: "cloud",
  });
  const [backup, setBackup] = useState<BackupView | null>(null);
  const [kinds, setKinds] = useState<SyncKind[] | null>(null);

  useEffect(() => {
    backupView().then(
      (v) => {
        setBackup(v);
        // A machine set up before (Replay First Run) starts from what it uses.
        if (v.lan) set({ method: "lan" });
        else if (v.github) set({ method: "github" });
      },
      () => {},
    );
    syncPrefsView().then(setKinds, () => {});
  }, []);

  function set(change: Partial<FirstRunAnswers>) {
    setAnswers((a) => ({ ...a, ...change }));
  }

  const at = STEPS.indexOf(step);
  const next = () => setStep(STEPS[Math.min(at + 1, STEPS.length - 1)]);
  const back = () => setStep(STEPS[Math.max(at - 1, 0)]);

  function finish() {
    // The questions stand in for the tour on a first run; it stays one click away.
    markTourSeen();
    onScan(answers);
  }

  const body: Record<StepId, { title: string; intro?: ReactNode; content: ReactNode }> = {
    machine: {
      title: "Let's set up Patch Pilot",
      intro: (
        <>
          Patch Pilot keeps every machine in your rig matching: plug-ins, DAW files and settings. A few quick
          questions, then it looks at what's on this machine.{" "}
          <button type="button" onClick={onTour} className="text-accent-text hover:underline">
            How it works
          </button>
        </>
      ),
      content: <MachineStep answers={answers} onChange={set} onEnter={next} />,
    },
    daws: {
      title: "Which DAWs do you use here?",
      intro: "Patch Pilot reads their templates, scripts, presets and settings. Untick one this machine doesn't have.",
      content: <DawStep answers={answers} onChange={set} />,
    },
    method: {
      title: "How should your machines share files?",
      intro:
        "New and changed templates, scripts and presets reach your other machines on their own. Plug-ins, sample libraries and licenses never copy: Patch Pilot lists what to install instead.",
      content: (
        <MethodStep
          answers={answers}
          onChange={set}
          backup={backup}
          onBackup={setBackup}
          onError={onError}
        />
      ),
    },
    what: {
      title: "What should stay the same on every machine?",
      intro:
        answers.method === "later"
          ? "These apply once you turn syncing on. Untick anything each machine should keep its own."
          : "Untick anything each machine should keep its own. Deleted files and audio or MIDI settings never sync.",
      content: <WhatStep answers={answers} kinds={kinds} onKinds={setKinds} onError={onError} />,
    },
    ready: {
      title: "Ready when you are",
      intro: "Here's what happens. Everything can be changed later in Settings.",
      content: <Summary answers={answers} backup={backup} />,
    },
  };
  const b = body[step];
  const last = step === "ready";

  return (
    <div className="mx-auto flex max-w-md flex-col gap-3 px-6 pb-6 pt-[8vh]">
      <div>
        <AppMark />
        <p className="text-[11px] text-dim">
          {at + 1} of {STEPS.length}
        </p>
        <h1 className="text-lg font-semibold">{b.title}</h1>
        {b.intro && <p className="mt-1 leading-relaxed text-dim">{b.intro}</p>}
      </div>
      {/* Not a form: the folder picker and GitHub sign-in inside have buttons of their own. */}
      <div className="flex flex-col gap-3">
        {b.content}
        <div className="flex items-center gap-1">
          {at > 0 && (
            <Button type="button" variant="quiet" onClick={back}>
              Back
            </Button>
          )}
          <span className="flex flex-1 justify-center gap-1.5" aria-hidden="true">
            {STEPS.map((s) => (
              <span key={s} className={`h-1.5 w-1.5 rounded-full ${s === step ? "bg-accent" : "bg-track"}`} />
            ))}
          </span>
          {!last && step !== "machine" && (
            <Button type="button" variant="quiet" onClick={() => setStep("ready")}>
              Skip to the end
            </Button>
          )}
          <Button type="button" onClick={last ? finish : next}>
            {last ? "Scan this machine" : "Next"}
          </Button>
        </div>
      </div>
    </div>
  );
}

function MachineStep({
  answers,
  onChange,
  onEnter,
}: {
  answers: FirstRunAnswers;
  onChange: (a: Partial<FirstRunAnswers>) => void;
  onEnter: () => void;
}) {
  return (
    <>
      <div className="flex flex-col gap-1">
        <label className="text-xs text-dim" htmlFor="machine-name">
          Name this machine
        </label>
        <input
          id="machine-name"
          autoFocus
          value={answers.name}
          onChange={(e) => onChange({ name: e.target.value })}
          onKeyDown={(e) => e.key === "Enter" && onEnter()}
          placeholder="Studio PC"
          className={inputClass}
        />
      </div>
      <div className="flex flex-col gap-1">
        <span className="text-xs text-dim">Is it your main machine?</span>
        <Panel className="flex flex-col gap-0.5 p-1">
          <ChoiceRow
            group="role"
            icon="star"
            name="Yes, it's my main rig"
            lines={["It becomes your Primary Rig: the setup every other machine is measured against."]}
            checked={answers.role === "main"}
            onPick={() => onChange({ role: "main" })}
          />
          <ChoiceRow
            group="role"
            icon="laptop"
            name="No, my main machine already has Patch Pilot"
            lines={["After the scan you link the two with a short code, and this one catches up."]}
            checked={answers.role === "other"}
            onPick={() => onChange({ role: "other" })}
          />
          <ChoiceRow
            group="role"
            icon="question"
            name="Not sure yet"
            lines={["Pick a Primary Rig later from This machine."]}
            checked={answers.role === "later"}
            onPick={() => onChange({ role: "later" })}
          />
        </Panel>
      </div>
    </>
  );
}

function DawStep({ answers, onChange }: { answers: FirstRunAnswers; onChange: (a: Partial<FirstRunAnswers>) => void }) {
  return (
    <Panel className="flex flex-col gap-0.5 p-1">
      <ChoiceRow
        type="checkbox"
        icon="sliders"
        name="Reaper"
        lines={["Track and project templates, FX chains, scripts and JSFX, themes, shortcuts, menus."]}
        checked={answers.reaper}
        onPick={(on) => onChange({ reaper: on })}
      />
      <ChoiceRow
        type="checkbox"
        icon="layers"
        name="Ableton Live"
        lines={["Templates, racks and presets, Max for Live devices, Collections and Places."]}
        checked={answers.live}
        onPick={(on) => onChange({ live: on })}
      />
      {!answers.reaper && !answers.live && (
        <p className="px-2 py-1 text-[11px] text-dim">
          Neither: Patch Pilot still keeps your plug-ins and plug-in presets matching.
        </p>
      )}
    </Panel>
  );
}

function MethodStep({
  answers,
  onChange,
  backup,
  onBackup,
  onError,
}: {
  answers: FirstRunAnswers;
  onChange: (a: Partial<FirstRunAnswers>) => void;
  backup: BackupView | null;
  onBackup: (v: BackupView) => void;
  onError: (e: string) => void;
}) {
  const [busy, setBusy] = useState(false);
  const [changing, setChanging] = useState(false);

  async function chooseFolder(service: string | null, folder: string | null) {
    setBusy(true);
    try {
      await setBackupFolder(service, folder);
      await setKeepInSync(true);
      onBackup(await backupView());
      setChanging(false);
    } catch (e) {
      onError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function usedGitHub() {
    try {
      await setKeepInSync(true);
      onBackup(await backupView());
    } catch (e) {
      onError(String(e));
    }
  }

  const cloud = cloudInUse(backup);
  const setup =
    answers.method === "cloud" && backup ? (
      cloud && !changing ? (
        <Done
          text={`Syncing through ${cloud}.`}
          action={
            <Button type="button" variant="quiet" className="h-6 px-2" onClick={() => setChanging(true)}>
              Change
            </Button>
          }
        />
      ) : (
        <FolderPicker
          clouds={backup.clouds}
          busy={busy}
          missing={false}
          preferred={backup.settings.folder ?? backup.transfer_folder}
          onChoose={chooseFolder}
          onCancel={cloud ? () => setChanging(false) : undefined}
          hint="Pick the same service on each machine. Patch Pilot keeps a Patch Pilot Backup folder there."
        />
      )
    ) : answers.method === "github" && backup ? (
      backup.github && backup.settings.keep_in_sync ? (
        <Done text={`Syncing through GitHub (${backup.github}).`} />
      ) : (
        <GitHubSetup bare onUsed={usedGitHub} onCancel={() => onChange({ method: "cloud" })} onError={onError} />
      )
    ) : answers.method === "lan" ? (
      <p className="text-[11px] text-dim">
        {backup?.lan
          ? "Already syncing over your home network."
          : "Turns on once this machine is linked to your others, right after the scan. Your computer may ask to allow Patch Pilot on the local network."}
      </p>
    ) : null;

  return (
    <Panel className="flex flex-col gap-0.5 p-1">
      {SYNC_METHODS.map((m) => (
        <div key={m.id}>
          <ChoiceRow
            group="method"
            icon={m.icon}
            name={m.name}
            lines={[m.good, m.limit]}
            checked={answers.method === m.id}
            onPick={() => onChange({ method: m.id })}
          />
          {answers.method === m.id && setup && <div className="mb-1 ml-[38px] mr-2 mt-0.5">{setup}</div>}
        </div>
      ))}
      <ChoiceRow
        group="method"
        icon="clock"
        name="Decide later"
        lines={["Patch Pilot still compares your machines. Set up syncing any time in Settings."]}
        checked={answers.method === "later"}
        onPick={() => onChange({ method: "later" })}
      />
    </Panel>
  );
}

function Done({ text, action }: { text: string; action?: ReactNode }) {
  return (
    <span className="flex items-center gap-2 text-xs text-ok">
      ✓ {text}
      {action}
    </span>
  );
}

/** The cloud service in use, when syncing goes through a cloud folder (not GitHub or the home network). */
function cloudInUse(v: BackupView | null): string | null {
  if (!v?.dir || !v.settings.keep_in_sync || v.github || v.lan) return null;
  return v.clouds.find((c) => c.path === v.settings.folder)?.name ?? "your folder";
}

/**
 * Choose what syncs in a few plain groups rather than 25 kinds. A group is
 * ticked while all of its kinds for the DAWs picked are on; Settings has the
 * full list for anything finer.
 */
const GROUPS: { id: string; label: string; kinds: string[] }[] = [
  { id: "templates", label: "Templates", kinds: ["reaper/track-templates", "reaper/project-templates", "ableton/templates"] },
  {
    id: "scripts",
    label: "Scripts and devices",
    kinds: ["reaper/scripts", "reaper/startup-action", "ableton/m4l", "ableton/remote-scripts"],
  },
  {
    id: "presets",
    label: "Presets, chains and racks",
    kinds: [
      "presets",
      "reaper/fx-chains",
      "reaper/fx-presets",
      "ableton/defaults",
      "ableton/presets",
      "ableton/clips",
      "ableton/grooves",
      "ableton/tunings",
    ],
  },
  { id: "keys", label: "Shortcuts, menus and mouse", kinds: ["reaper/key-map", "reaper/menus", "reaper/mouse"] },
  { id: "look", label: "Themes", kinds: ["reaper/themes", "reaper/theme-settings", "reaper/toolbar-icons"] },
  {
    id: "library",
    label: "Library layout",
    kinds: ["ableton/collections", "ableton/tags", "ableton/places", "ableton/options"],
  },
];

const GROUP_ICON = {
  templates: "doc",
  scripts: "code",
  presets: "knob",
  keys: "key",
  look: "sun",
  library: "folder",
} as const;

function forDaws(kinds: SyncKind[], a: FirstRunAnswers): SyncKind[] {
  return kinds.filter((k) => (k.app === "Reaper" ? a.reaper : k.app === "Ableton Live" ? a.live : true));
}

function WhatStep({
  answers,
  kinds,
  onKinds,
  onError,
}: {
  answers: FirstRunAnswers;
  kinds: SyncKind[] | null;
  onKinds: (k: SyncKind[]) => void;
  onError: (e: string) => void;
}) {
  const shown = useMemo(() => (kinds ? forDaws(kinds, answers) : []), [kinds, answers]);
  if (!kinds) return null;

  async function toggle(ids: string[], on: boolean) {
    onKinds(kinds!.map((k) => (ids.includes(k.id) ? { ...k, on } : k)));
    try {
      let latest = kinds!;
      for (const id of ids) latest = await setSyncKind(id, on);
      onKinds(latest);
    } catch (e) {
      onError(String(e));
    }
  }

  const groups = GROUPS.map((g) => ({ ...g, members: shown.filter((k) => g.kinds.includes(k.id)) })).filter(
    (g) => g.members.length > 0,
  );
  return (
    <Panel className="flex flex-col gap-0.5 p-1">
      {groups.map((g) => (
        <ChoiceRow
          key={g.id}
          type="checkbox"
          icon={GROUP_ICON[g.id as keyof typeof GROUP_ICON] ?? "copies"}
          name={g.label}
          lines={[g.members.map((k) => k.label).join(", ")]}
          checked={g.members.every((k) => k.on)}
          onPick={(on) => toggle(g.members.map((k) => k.id), on)}
        />
      ))}
      <p className="px-2 py-1 text-[11px] text-dim">
        Shortcuts move between PC and Mac keys on their own. Settings › What syncs has each kind on its own.
      </p>
    </Panel>
  );
}

function Summary({ answers, backup }: { answers: FirstRunAnswers; backup: BackupView | null }) {
  const daws = [answers.reaper && "Reaper", answers.live && "Ableton Live"].filter(Boolean) as string[];
  const cloud = cloudInUse(backup);
  const synced =
    answers.method === "cloud"
      ? cloud
        ? { tone: "ok" as const, text: `Files stay in sync through ${cloud}.` }
        : { tone: "warn" as const, text: "No cloud folder picked yet. Go Back, or pick one later in Settings." }
      : answers.method === "github"
        ? backup?.github && backup.settings.keep_in_sync
          ? { tone: "ok" as const, text: `Files stay in sync through GitHub (${backup.github}).` }
          : { tone: "warn" as const, text: "GitHub isn't set up yet. Go Back, or finish later in Settings." }
        : answers.method === "lan"
          ? { tone: "accent" as const, text: "Files sync over your home network once linked." }
          : { tone: "dim" as const, text: "Syncing stays off. Turn it on in Settings › Sync." };
  return (
    <Panel className="p-1">
      <Notice icon="search" text={`Reads your plug-ins${daws.length ? ` and ${daws.join(" and ")}` : ""}, read only.`} />
      {answers.role === "main" && <Notice tone="ok" icon="star" text="Makes this machine your Primary Rig." />}
      {(answers.role === "other" || answers.method === "lan") && (
        <Notice icon="link" text="Then links this machine to your others with a code." />
      )}
      <Notice tone={synced.tone} icon="sync" text={synced.text} />
      <Notice tone="dim" icon="plug" text="Get Up To Speed then lists what other machines need." />
    </Panel>
  );
}

/**
 * After the first scan: the answers that need a scanned machine. Returns the
 * lines to show, and whether to open the Link dialog (and on which side).
 */
export async function applyFirstRun(
  a: FirstRunAnswers,
  overview: Overview,
): Promise<{ lines: string[]; link: "show" | "enter" | null }> {
  const lines: string[] = [];
  const kinds = await syncPrefsView().catch(() => [] as SyncKind[]);
  for (const [daw, app, on, name] of [
    ["reaper", "Reaper", a.reaper, "Reaper"],
    ["live", "Ableton Live", a.live, "Ableton Live"],
  ] as const) {
    if (on) continue;
    await setDawWatched(daw, false);
    for (const k of kinds.filter((k) => k.app === app && k.on)) await setSyncKind(k.id, false);
    lines.push(`${name} isn't watched here, and its files don't sync to this machine. Change it in Settings.`);
  }
  if (a.role === "main") {
    if (!overview.golden) {
      await makeGolden();
      lines.push("This machine is your Primary Rig. Compare measures your other machines against it.");
    } else if (!overview.golden.from_this_machine) {
      lines.push("You already have a Primary Rig. Make this machine it from This machine if you like.");
    }
  }
  if (a.method === "lan") {
    try {
      localStorage.setItem(LAN_KEY, "pending");
    } catch {
      // Blocked storage: Home network… under Settings › Sync still turns it on.
    }
  }
  const link = a.role === "other" ? "enter" : a.method === "lan" ? "show" : null;
  return { lines, link };
}

/**
 * The home network answer, once this machine is linked: sync over it and keep
 * files in sync. Returns a line to show, or null when nothing was waiting.
 */
export async function finishLanIfPending(): Promise<string | null> {
  let pending = false;
  try {
    pending = localStorage.getItem(LAN_KEY) === "pending";
  } catch {
    return null;
  }
  if (!pending) return null;
  await lanUse();
  await setKeepInSync(true);
  try {
    localStorage.removeItem(LAN_KEY);
  } catch {
    // Already used; a second try only says so again.
  }
  return "Linked, and syncing over your home network. Files from your other machines arrive while both are on.";
}
