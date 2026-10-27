import { useCallback, useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  setupClose,
  setupFinish,
  setupReportText,
  setupSaveReport,
  setupStart,
  setupTick,
  setupView,
  ignoreItems,
  openVendorApp,
  openWebPage,
  vaultMatches,
  vaultRun,
  vendorApps,
  type OtherFormats,
  type SetupStep,
  type SetupView,
  type VaultMatch,
  type VendorApp,
} from "../api";
import { RestoreFromBackup } from "../Backup";
import { BigFiles } from "../BigFiles";
import { BringOver } from "../BringOver";
import { ReaperExtras } from "../ReaperExtras";
import { IgnoreButton, IgnoredToggle, productRules, useIgnores } from "../Ignore";
import { AppButton } from "./Compare";
import { Button, Folder, Label, Notice, scoreColor, timeAgo, useFolders } from "../ui";

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
  if (!view.active) return <Start view={view} onStart={(id) => run(setupStart(id))} onError={onError} />;
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
      onTick={(key, done) =>
        setupTick(key, done).then(apply, (e) => {
          onError(String(e));
          throw e;
        })
      }
      onFinish={() => run(setupFinish())}
      onReport={() => setupReportText().then(setReport, (e) => onError(String(e)))}
      onClose={() => run(setupClose())}
      onError={onError}
    />
  );
}

function Start({
  view,
  onStart,
  onError,
}: {
  view: SetupView;
  onStart: (profile: string | null) => void;
  onError: (e: string) => void;
}) {
  const [fromBackup, setFromBackup] = useState(false);
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
              checked={!fromBackup && profile === p.id}
              onChange={() => {
                setProfile(p.id);
                setFromBackup(false);
              }}
            />
            <span className="font-medium">{p.name}</span>
            {p.id && hint[p.id] && <span className="text-xs text-dim">{hint[p.id]}</span>}
          </label>
        ))}
        <label className="flex items-baseline gap-2 rounded-md px-2 py-1.5 hover:bg-sel/50">
          <input
            type="radio"
            name="profile"
            className="accent-accent"
            checked={fromBackup}
            onChange={() => setFromBackup(true)}
          />
          <span className="font-medium">Sync with a backup</span>
          <span className="text-xs text-dim">Your files, from a cloud folder.</span>
        </label>
      </fieldset>
      {fromBackup ? (
        <div className="text-xs">
          <RestoreFromBackup onError={onError} />
        </div>
      ) : (
        <div>
          <Button onClick={() => onStart(profile)}>Start setup</Button>
        </div>
      )}
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
  onTick: (key: string, done: boolean) => Promise<void>;
  onFinish: () => void;
  onReport: () => void;
  onClose: () => void;
  onError: (e: string) => void;
}) {
  const a = view.active!;
  const p = a.progress;
  const quick = useQuickSteps(p.steps);
  const steps = quick.steps;
  const done = steps.filter(isFinished).length;
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
    ignore: (label, products) =>
      quick.hide(products, ignoreItems(null, label, productRules(products))).catch((e) => onError(String(e))),
  };
  const ignores = useIgnores(null, view);
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
            {done} of {p.total} steps · score was {p.score_before} · {a.profile ?? "Whole Primary Rig"} · Primary Rig
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
        <div className="h-full rounded-full bg-accent" style={{ width: `${p.total ? (done / p.total) * 100 : 100}%` }} />
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

      <StepList steps={steps} scope="setup" onTick={(key, d) => quick.tick(key, d, onTick(key, d))} actions={actions} />
      <OtherFormatsNote items={p.other_formats} />
      <StepExtras steps={steps} onError={onError} />
      <IgnoredToggle ignores={ignores} machine={null} onError={onError} />

      <div>
        <Label className="pb-0.5 pt-1">Templates</Label>
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
 * Mark done, Undo and Ignore show at once: recounting the checklist in Rust
 * takes a moment, so each change is applied here first and kept until the
 * recount agrees (or the call fails).
 */
export function useQuickSteps(steps: SetupStep[]) {
  const [ticks, setTicks] = useState<Map<string, boolean>>(new Map());
  const [hidden, setHidden] = useState<Set<string>>(new Set());

  // Forget what the recount now shows by itself.
  useEffect(() => {
    setTicks((t) => {
      const left = new Map([...t].filter(([key, d]) => !agrees(steps.find((s) => s.key === key), d)));
      return left.size === t.size ? t : left;
    });
    setHidden((h) => {
      const open = new Set(steps.flatMap((s) => s.open_products));
      const left = new Set([...h].filter((id) => open.has(id)));
      return left.size === h.size ? h : left;
    });
  }, [steps]);

  const shown = useMemo(
    () => (ticks.size || hidden.size ? steps.map((s) => quickStep(s, ticks.get(s.key), hidden)) : steps),
    [steps, ticks, hidden],
  );
  return {
    steps: shown,
    tick(key: string, done: boolean, call: Promise<unknown>) {
      setTicks((t) => new Map(t).set(key, done));
      call.catch(() =>
        setTicks((t) => {
          const left = new Map(t);
          left.delete(key);
          return left;
        }),
      );
    },
    hide(products: string[], call: Promise<unknown>): Promise<unknown> {
      setHidden((h) => new Set([...h, ...products]));
      return call.catch((e) => {
        setHidden((h) => new Set([...h].filter((id) => !products.includes(id))));
        throw e;
      });
    },
  };
}

/** True when the recounted step already shows the tick (or its undo). */
function agrees(s: SetupStep | undefined, done: boolean): boolean {
  if (!s) return true;
  return done ? isFinished(s) : s.status !== "skipped" && (s.automatic || s.status !== "done");
}

/** A step as it will look once ticked (`tick`) and with `hidden` products ignored. */
function quickStep(s: SetupStep, tick: boolean | undefined, hidden: Set<string>): SetupStep {
  let step = s;
  if (s.automatic && s.open_products.some((id) => hidden.has(id))) {
    const keep = s.open_products.map((id) => !hidden.has(id));
    const open_products = s.open_products.filter((_, n) => keep[n]);
    step = {
      ...s,
      open: s.open.filter((_, n) => keep[n] ?? true),
      open_products,
      open_needs: s.open_needs.filter((_, n) => keep[n] ?? true),
      status: open_products.length === 0 ? "done" : s.status,
    };
  }
  if (tick === undefined || agrees(step, tick)) return step;
  if (!step.automatic) return { ...step, status: tick ? "done" : "todo" };
  if (tick) return { ...step, status: "skipped" };
  return { ...step, status: step.finished.length ? "partly" : "todo" };
}

/**
 * Plug-ins here in other formats than the Primary Rig's (a VST3 where the PC
 * has a VST2). They count as installed; this says what differs, folded away.
 */
export function OtherFormatsNote({ items }: { items: OtherFormats[] }) {
  const [open, setOpen] = useState(false);
  if (!items.length) return null;
  const n = items.length;
  return (
    <div className="-mx-2 text-xs">
      <Folder
        title={`${n} plug-in${n === 1 ? " is" : "s are"} here in another format`}
        count={n}
        className="px-2"
        open={open}
        onToggle={() => setOpen((o) => !o)}
        badges={<span className="text-[11px] text-dim">Counted as installed</span>}
      >
        <ul className="pl-6 pr-2">
          {items.map((o) => (
            <li key={o.product_id} className="flex gap-2 py-0.5">
              <span className="min-w-0 truncate">{o.name}</span>
              <span className="shrink-0 text-dim">
                {o.here.join(", ")} here · Primary Rig also has {o.missing.join(", ")}
              </span>
            </li>
          ))}
        </ul>
      </Folder>
    </div>
  );
}

/** The step Bring over and Big files sit in (parity's plan: "Bring over N DAW files ..."). */
const isBringStep = (s: SetupStep) => s.automatic && s.title.startsWith("Bring over");
/** ReaPack's and Reaper's folder settings steps, where Reaper setup sits. */
const isReaperStep = (s: SetupStep) =>
  s.automatic && (s.title.includes("ReaPack item") || s.title.includes("Reaper folder setting"));

/**
 * Bring over, Big files and Reaper setup when the checklist has no open step
 * for them: the "Up to date" line, or files the plan doesn't count. Otherwise
 * they sit inside their step.
 */
export function StepExtras({ steps, onError }: { steps: SetupStep[]; onError: (e: string) => void }) {
  const open = steps.filter((s) => !isFinished(s));
  return (
    <div className="flex flex-col gap-2">
      {!open.some(isBringStep) && (
        <>
          <BringOver onError={onError} />
          <BigFiles onError={onError} />
        </>
      )}
      {!open.some(isReaperStep) && <ReaperExtras onError={onError} />}
    </div>
  );
}

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
  // Reaper setup covers both Reaper steps, so it shows once, in the first.
  const reaperKey = steps.find((s) => !isFinished(s) && isReaperStep(s))?.key;
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
                  <StepRow step={s} onTick={onTick} actions={actions} reaper={s.key === reaperKey} />
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
  /** Show Bring over and Reaper setup inside their steps (this machine's own checklist). */
  bringOver: boolean;
  /** Stop asking for these products on the machine (decision #65). */
  ignore?: (label: string, products: string[]) => void;
}

/** Items listed under a step before "Show all". */
const SHOWN = 5;

/** `fabfilter.com` for `https://www.fabfilter.com`. */
function siteName(url: string): string {
  try {
    return new URL(url).hostname.replace(/^www\./, "");
  } catch {
    return url;
  }
}

/** A web search for a plug-in whose vendor Patch Pilot doesn't know. */
function searchUrl(line: string): string {
  return `https://duckduckgo.com/?q=${encodeURIComponent(`${line} plugin download`)}`;
}

export function StepRow({
  step: s,
  onTick,
  actions,
  reaper = false,
}: {
  step: SetupStep;
  onTick: (key: string, done: boolean) => void;
  actions: Actions;
  /** Show Reaper setup in this step. */
  reaper?: boolean;
}) {
  const finished = isFinished(s);
  const mark = MARK[s.status];
  // Plug-in and app lines say whether they're missing or older; DAW files and Reaper settings don't.
  const tagged = s.automatic && !isBringStep(s) && !isReaperStep(s);
  const [showAll, setShowAll] = useState(false);
  const web = (url: string) => openWebPage(url).catch((e) => actions.onError(String(e)));
  const box = (
    <span
      aria-hidden="true"
      className={`mt-0.5 flex h-3.5 w-3.5 shrink-0 items-center justify-center rounded-[4px] text-[9px] font-bold ${mark.className}`}
    >
      {mark.icon}
    </span>
  );
  return (
    <div className="group/step flex gap-2.5 rounded-[5px] px-2 py-1.5 hover:bg-sel/50">
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
              const need = tagged ? s.open_needs[n] : undefined;
              return (
                <li key={d} className="group flex items-center gap-2">
                  {need && (
                    <span
                      className={`w-[72px] shrink-0 text-[10px] ${need === "update" ? "text-warn" : "text-dim"}`}
                      title={
                        need === "update"
                          ? "Installed here, but an older version than the Primary Rig's"
                          : "Not installed here in any format"
                      }
                    >
                      {need === "update" ? "Update" : "Not installed"}
                    </span>
                  )}
                  <span className="min-w-0 truncate">{d}</span>
                  {actions.ignore && s.automatic && s.open_products[n] && (
                    <IgnoreButton reveal="step" onIgnore={() => actions.ignore!(d, [s.open_products[n]])} />
                  )}
                  {installer && (
                    <button
                      className="shrink-0 text-accent-text hover:underline"
                      title={`From your Installer Vault: ${installer.file_name}`}
                      onClick={() => actions.run(installer.entry_id)}
                    >
                      Run installer{installer.version ? ` ${installer.version}` : ""}
                    </button>
                  )}
                  {!installer && s.search && (
                    <button
                      className="shrink-0 text-accent-text opacity-0 hover:underline focus:opacity-100 group-hover:opacity-100"
                      title="Patch Pilot doesn't know this plug-in's vendor. Search the web for it."
                      onClick={() => web(searchUrl(d))}
                    >
                      Search
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
        {!finished && actions.bringOver && isBringStep(s) && (
          <div className="mt-1.5 flex flex-col gap-2">
            <BringOver onError={actions.onError} />
            <BigFiles onError={actions.onError} />
          </div>
        )}
        {!finished && actions.bringOver && reaper && (
          <div className="mt-1.5">
            <ReaperExtras onError={actions.onError} />
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
      {!finished && s.link && (
        <Button
          variant="quiet"
          className="shrink-0 self-start"
          title={`Opens ${s.link}, where the vendor offers its downloads`}
          onClick={() => web(s.link!)}
        >
          Get from {siteName(s.link)}
        </Button>
      )}
      {actions.ignore && s.automatic && !finished && s.open_products.length > 0 && (
        <button
          onClick={() => actions.ignore!(s.title, s.open_products)}
          className="h-6 shrink-0 self-start rounded-[5px] px-2 text-[11px] text-dim opacity-0 hover:bg-sel hover:text-fg focus-visible:opacity-100 group-hover/step:opacity-100"
          title="Stop asking for what's left in this step on this machine. It no longer counts against the score."
        >
          Ignore
        </button>
      )}
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

/** A line on "This machine" offering setup, when this machine isn't at parity yet. */
export function SetupOffer({ onOpen }: { onOpen: () => void }) {
  return (
    <Notice
      text="New machine? Patch Pilot can walk you through setting it up to match your Primary Rig."
      actions={
        <Button onClick={onOpen} className="h-6 shrink-0 px-2.5">
          Set up this machine
        </Button>
      }
    />
  );
}
