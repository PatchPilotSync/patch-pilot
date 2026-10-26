import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  setupClose,
  setupFinish,
  setupReportText,
  setupSaveReport,
  setupStart,
  setupTick,
  setupView,
  openVendorApp,
  vaultMatches,
  vaultRun,
  vendorApps,
  type SetupStep,
  type SetupView,
  type VaultMatch,
  type VendorApp,
} from "../api";
import { BigFiles } from "../BigFiles";
import { BringOver } from "../BringOver";
import { AppButton } from "./Compare";
import { Button, Folder, scoreColor, timeAgo, useFolders } from "../ui";

interface Props {
  onRescan: () => void;
  onError: (e: string) => void;
  /** Tells the app the view changed, so the top bar can show or hide the tab. */
  onChange: (v: SetupView) => void;
}

/** New-machine setup: choose a profile, follow the phased plan, get a report. */
export function Setup({ onRescan, onError, onChange }: Props) {
  const [view, setView] = useState<SetupView | null>(null);
  const [report, setReport] = useState<string | null>(null);

  const apply = useCallback(
    (v: SetupView) => {
      setView(v);
      onChange(v);
    },
    [onChange],
  );
  const run = useCallback(
    (p: Promise<SetupView>) => p.then(apply, (e) => onError(String(e))),
    [apply, onError],
  );

  useEffect(() => {
    run(setupView());
    // Background rescans (plug-in folders changed) move the progress on.
    const unlisten = listen("data-changed", () => run(setupView()));
    return () => {
      unlisten.then((f) => f());
    };
  }, [run]);

  useEffect(() => {
    if (view?.active?.finished_at) setupReportText().then(setReport, (e) => onError(String(e)));
  }, [view, onError]);

  if (!view) return null;
  if (!view.active) return <Start view={view} onStart={(id) => run(setupStart(id))} />;
  if (report !== null && view.active.finished_at)
    return (
      <Report
        text={report}
        onBack={() => setReport(null)}
        onClose={() => run(setupClose())}
        onError={onError}
      />
    );
  return (
    <Progress
      view={view}
      onRescan={onRescan}
      onTick={(key, done) => run(setupTick(key, done))}
      onFinish={() => run(setupFinish())}
      onReport={() => setupReportText().then(setReport, (e) => onError(String(e)))}
      onClose={() => run(setupClose())}
      onError={onError}
    />
  );
}

function Start({ view, onStart }: { view: SetupView; onStart: (profile: string | null) => void }) {
  // Studio clone has no rules, so it stands for "everything on the Primary Rig".
  const [profile, setProfile] = useState<string | null>(
    view.profile_id ?? view.profiles.find((p) => p.id === "studio-clone")?.id ?? null,
  );
  const choices: { id: string | null; name: string }[] = view.profiles.length
    ? view.profiles
    : [{ id: null, name: "Whole Primary Rig" }];
  const hint: Record<string, string> = {
    "studio-clone": "Everything on the Primary Rig.",
    travel: "Leaves out AAX and anything over 20 GB.",
  };
  return (
    <div className="mx-auto flex max-w-lg flex-col gap-4 px-6 py-8">
      <div>
        <h1 className="text-base font-semibold">Set up this machine</h1>
        <p className="mt-1 text-xs leading-relaxed text-dim">
          Patch Pilot lists what this machine needs from your Primary Rig, in the order to do it: licences and
          vendor apps first, then plug-ins, then your DAW files. It never installs or copies plug-ins for you. As
          you install things, it ticks them off after each rescan, and you get a report at the end.
        </p>
      </div>
      <fieldset className="flex flex-col gap-1">
        <legend className="mb-1 text-[11px] text-dim">What should this machine have?</legend>
        {choices.map((p) => (
          <label key={p.id ?? ""} className="flex items-baseline gap-2 rounded-md px-2 py-1.5 hover:bg-sel/50">
            <input
              type="radio"
              name="profile"
              className="accent-accent"
              checked={profile === p.id}
              onChange={() => setProfile(p.id)}
            />
            <span className="font-medium">{p.name}</span>
            {p.id && hint[p.id] && <span className="text-xs text-dim">{hint[p.id]}</span>}
          </label>
        ))}
      </fieldset>
      <div>
        <Button onClick={() => onStart(profile)}>Start setup</Button>
      </div>
    </div>
  );
}

function Progress({
  view,
  onRescan,
  onTick,
  onFinish,
  onReport,
  onClose,
  onError,
}: {
  view: SetupView;
  onRescan: () => void;
  onTick: (key: string, done: boolean) => void;
  onFinish: () => void;
  onReport: () => void;
  onClose: () => void;
  onError: (e: string) => void;
}) {
  const a = view.active!;
  const p = a.progress;
  // Open buttons for vendor apps, and installers kept in the Installer Vault.
  const [apps, setApps] = useState<VendorApp[]>([]);
  const [installers, setInstallers] = useState<Map<string, VaultMatch>>(new Map());
  const wanted = p.steps.flatMap((s) => s.open_products).join("\n");
  useEffect(() => {
    vendorApps().then(setApps, () => {});
    const products = wanted ? wanted.split("\n") : [];
    vaultMatches(products).then(
      (m) => setInstallers(new Map(m.map((x) => [x.product_id, x]))),
      () => {},
    );
  }, [wanted]);
  const actions: Actions = {
    app: (id) => apps.find((x) => x.id === id),
    installer: (product) => installers.get(product),
    open: (id) => openVendorApp(id).then(() => {}, (e) => onError(String(e))),
    run: (entry) => vaultRun(entry).catch((e) => onError(String(e))),
    onError,
    bringOver: true,
  };
  const notReady = p.templates.filter((t) => t.missing.length > 0);
  const [templatesOpen, setTemplatesOpen] = useState(false);
  return (
    <div className="flex flex-col gap-3 px-5 py-4">
      <header className="flex items-center gap-4">
        <span className={`text-3xl font-semibold leading-none tracking-tight tabular-nums ${scoreColor(p.score_now)}`}>
          {p.score_now}
        </span>
        <div className="min-w-0 flex-1">
          <h1 className="truncate text-sm font-semibold">Setting up {a.machine}</h1>
          <p className="truncate text-xs text-dim">
            {p.done} of {p.total} steps · score was {p.score_before} · {a.profile ?? "Whole Primary Rig"} · Primary Rig
            v{a.golden_version} · started {timeAgo(a.started_at)}
          </p>
        </div>
        <Button variant="quiet" onClick={onReport}>
          Report
        </Button>
        <Button variant="quiet" onClick={onRescan}>
          Rescan
        </Button>
      </header>
      <div className="h-[3px] overflow-hidden rounded-full bg-track" aria-hidden="true">
        <div className="h-full rounded-full bg-accent" style={{ width: `${p.total ? (p.done / p.total) * 100 : 100}%` }} />
      </div>

      {p.complete && !a.finished_at ? (
        <div className="flex items-center gap-3 rounded-md bg-ok/10 px-3 py-2 text-xs">
          <span className="flex-1">
            <span className="font-medium text-ok">This machine is ready.</span>{" "}
            <span className="text-dim">Every step is done and every template has its plug-ins.</span>
          </span>
          <Button onClick={onFinish}>Finish and see the report</Button>
        </div>
      ) : (
        <p className="text-xs text-dim">
          Work down the list. Patch Pilot rescans when your plug-in folders change, or press Rescan.
        </p>
      )}

      <StepList steps={p.steps} scope="setup" onTick={onTick} actions={actions} />

      <div>
        <h3 className="pb-0.5 pt-1 text-[11px] text-dim">Templates</h3>
        {p.templates.length === 0 ? (
          <p className="text-xs text-dim">No templates on this machine yet.</p>
        ) : notReady.length === 0 ? (
          <p className="text-xs text-ok">All {p.templates.length} templates have every plug-in they use.</p>
        ) : (
          <div className="-mx-2 text-xs">
            <Folder
              title={`${notReady.length} still missing plug-ins`}
              count={p.templates.length}
              className="px-2"
              open={templatesOpen}
              onToggle={() => setTemplatesOpen((o) => !o)}
              badges={
                <span className="text-[11px] text-dim">
                  {p.templates_ready} of {p.templates.length} ready
                </span>
              }
            >
              <ul className="pl-6 pr-2">
                {notReady.map((t) => (
                  <li key={t.product_id} className="truncate py-0.5">
                    {t.name} <span className="text-dim">needs {t.missing.length}</span>
                  </li>
                ))}
              </ul>
            </Folder>
          </div>
        )}
      </div>

      <div className="pt-2">
        <Button variant="quiet" onClick={onClose} className="-ml-3">
          Stop setup
        </Button>
      </div>
    </div>
  );
}

const isFinished = (s: SetupStep) => s.status === "done" || s.status === "skipped";

/**
 * Checklist steps in collapsible groups. A group with steps left starts open;
 * a finished one folds away, unless the user has opened or closed it.
 */
export function StepList({
  steps,
  scope,
  onTick,
  actions,
}: {
  steps: SetupStep[];
  /** Keeps remembered open/closed state apart per checklist. */
  scope: string;
  onTick: (key: string, done: boolean) => void;
  actions: Actions;
}) {
  const groups: [string, SetupStep[]][] = [];
  for (const s of steps) {
    const last = groups[groups.length - 1];
    if (last && last[0] === s.group) last[1].push(s);
    else groups.push([s.group, [s]]);
  }
  const folders = useFolders(`steps.${scope}`, (g) => groups.find(([n]) => n === g)?.[1].some((s) => !isFinished(s)) ?? true);
  return (
    <div className="-mx-2 flex flex-col">
      {groups.map(([group, ss]) => {
        const done = ss.filter(isFinished).length;
        return (
          <Folder
            key={group}
            title={group}
            count={ss.length}
            className="px-2"
            open={folders.isOpen(group)}
            onToggle={() => folders.toggle(group)}
            badges={
              <span className={`text-[11px] tabular-nums ${done === ss.length ? "text-ok" : "text-dim"}`}>
                {done === ss.length ? "Done" : `${done} of ${ss.length}`}
              </span>
            }
          >
            <ol className="flex flex-col pl-4">
              {ss.map((s) => (
                <li key={s.key}>
                  <StepRow step={s} onTick={onTick} actions={actions} />
                </li>
              ))}
            </ol>
          </Folder>
        );
      })}
    </div>
  );
}

const MARK: Record<SetupStep["status"], { icon: string; className: string }> = {
  done: { icon: "✓", className: "bg-ok text-on-accent" },
  skipped: { icon: "✓", className: "bg-dim text-on-accent" },
  partly: { icon: "", className: "border border-warn bg-warn/20" },
  todo: { icon: "", className: "border border-line" },
};

export interface Actions {
  app: (id: string) => VendorApp | undefined;
  installer: (product: string) => VaultMatch | undefined;
  open: (id: string) => void;
  run: (entry: string) => void;
  onError: (e: string) => void;
  /** Show Bring over inside its step (this machine's own checklist). */
  bringOver: boolean;
}

/** Items listed under a step before "Show all". */
const SHOWN = 5;

export function StepRow({
  step: s,
  onTick,
  actions,
}: {
  step: SetupStep;
  onTick: (key: string, done: boolean) => void;
  actions: Actions;
}) {
  const finished = isFinished(s);
  const mark = MARK[s.status];
  const [showAll, setShowAll] = useState(false);
  const box = (
    <span
      aria-hidden="true"
      className={`mt-0.5 flex h-3.5 w-3.5 shrink-0 items-center justify-center rounded-[4px] text-[9px] font-bold ${mark.className}`}
    >
      {mark.icon}
    </span>
  );
  return (
    <div className="flex gap-2.5 rounded-[5px] px-2 py-1.5 hover:bg-sel/50">
      {s.automatic ? (
        box
      ) : (
        <input
          type="checkbox"
          aria-label={s.title}
          className="mt-0.5 accent-accent"
          checked={finished}
          onChange={(e) => onTick(s.key, e.target.checked)}
        />
      )}
      <span className="min-w-0 flex-1">
        <span className={finished ? "text-dim line-through" : ""}>
          {s.number ? `${s.number}. ` : ""}
          {s.title}
        </span>
        {s.automatic && s.status === "done" && (
          <span className="ml-2 text-[11px] text-ok">{s.title.startsWith("Bring over") ? "Done" : "Installed"}</span>
        )}
        {!finished && s.open.length > 0 && (
          <ul className="mt-0.5 text-xs text-dim">
            {(showAll ? s.open : s.open.slice(0, SHOWN)).map((d, n) => {
              const installer = s.automatic ? actions.installer(s.open_products[n]) : undefined;
              return (
                <li key={d} className="flex items-center gap-2">
                  <span className="min-w-0 truncate">{d}</span>
                  {installer && (
                    <button
                      className="shrink-0 text-accent-text hover:underline"
                      title={`From your Installer Vault: ${installer.file_name}`}
                      onClick={() => actions.run(installer.entry_id)}
                    >
                      Run installer{installer.version ? ` ${installer.version}` : ""}
                    </button>
                  )}
                </li>
              );
            })}
            {s.open.length > SHOWN && (
              <li>
                <button className="text-accent-text hover:underline" onClick={() => setShowAll((v) => !v)}>
                  {showAll ? "Show fewer" : `Show all ${s.open.length}`}
                </button>
              </li>
            )}
          </ul>
        )}
        {s.automatic && s.finished.length > 0 && s.status === "partly" && (
          <ul className="mt-0.5 text-xs text-ok">
            {s.finished.length > SHOWN ? (
              <li title={s.finished.join("\n")}>✓ {s.finished.length} already done</li>
            ) : (
              s.finished.map((d) => <li key={d}>✓ {d}</li>)
            )}
          </ul>
        )}
        {!finished && actions.bringOver && s.automatic && s.title.startsWith("Bring over") && (
          <div className="mt-1.5 flex flex-col gap-2">
            <BringOver onError={actions.onError} />
            <BigFiles onError={actions.onError} />
          </div>
        )}
        {!finished && s.after.length > 0 && (
          <span className="mt-0.5 block text-[11px] text-dim opacity-80">
            After step{s.after.length === 1 ? "" : "s"} {s.after.join(", ")}
          </span>
        )}
        {s.status === "skipped" && <span className="mt-0.5 block text-[11px] text-dim">Marked done by you</span>}
      </span>
      {!finished && s.app && <AppButton app={actions.app(s.app)} onOpen={actions.open} />}
      {s.automatic && s.status !== "done" && (
        <button
          onClick={() => onTick(s.key, s.status !== "skipped")}
          className="h-6 shrink-0 self-start rounded-[5px] px-2 text-[11px] text-dim hover:bg-sel hover:text-fg"
          title={s.status === "skipped" ? undefined : "For things Patch Pilot can't see, or that you'll skip"}
        >
          {s.status === "skipped" ? "Undo" : "Mark done"}
        </button>
      )}
    </div>
  );
}

function Report({
  text,
  onBack,
  onClose,
  onError,
}: {
  text: string;
  onBack: () => void;
  onClose: () => void;
  onError: (e: string) => void;
}) {
  const [note, setNote] = useState<string | null>(null);
  return (
    <div className="flex flex-col gap-3 px-5 py-4">
      <header className="flex items-center gap-2">
        <h1 className="flex-1 text-sm font-semibold">Setup report</h1>
        <Button
          variant="quiet"
          onClick={() =>
            navigator.clipboard.writeText(text).then(() => setNote("Copied."), (e) => onError(String(e)))
          }
        >
          Copy
        </Button>
        <Button
          variant="quiet"
          onClick={() => setupSaveReport().then((p) => setNote(`Saved to ${p}`), (e) => onError(String(e)))}
        >
          Save to Downloads
        </Button>
        <Button variant="quiet" onClick={onBack}>
          Back
        </Button>
        <Button onClick={onClose}>Done</Button>
      </header>
      {note && <p className="text-xs text-dim">{note}</p>}
      <pre className="whitespace-pre-wrap rounded-md border border-line bg-panel p-3 font-mono text-[11px] leading-relaxed">
        {text}
      </pre>
    </div>
  );
}

/** A slim line on "This machine" offering setup, when this machine isn't at parity yet. */
export function SetupOffer({ onOpen }: { onOpen: () => void }) {
  return (
    <div className="flex shrink-0 items-center gap-3 border-b border-line bg-accent/10 px-4 py-1.5 text-xs">
      <span className="flex-1">
        New machine? Patch Pilot can walk you through setting it up to match your Primary Rig.
      </span>
      <Button onClick={onOpen}>Set up this machine</Button>
    </div>
  );
}
