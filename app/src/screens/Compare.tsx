import { useEffect, useMemo, useState } from "react";
import {
  compareMachine,
  FORMAT_LABEL,
  PHASE_LABEL,
  planMachine,
  STATE_HINT,
  STATE_LABEL,
  type ComparisonRow,
  type MachineCard,
  type MachineComparison,
  type MachinesView,
  type ParityState,
  type PluginFormat,
  type Step,
} from "../api";
import { BringOver } from "../BringOver";
import { Badge, Button, inputClass, Pill, ScoreBar, scoreColor, timeAgo } from "../ui";

const STATE_ORDER: ParityState[] = [
  "missing",
  "incomplete",
  "outdated",
  "modified",
  "version_unknown",
  "newer_than_golden",
  "extra",
  "not_applicable",
  "in_parity",
];

/** Status dot colour for each parity state. */
const STATE_DOT: Record<ParityState, string> = {
  missing: "bg-bad",
  incomplete: "bg-warn",
  outdated: "bg-warn",
  modified: "bg-warn",
  version_unknown: "bg-warn",
  newer_than_golden: "bg-ok",
  extra: "bg-dim",
  not_applicable: "bg-dim",
  in_parity: "bg-ok",
};

/** A request, from the tray popover, to show one machine (and maybe its plan). */
export interface Focus {
  id: string;
  plan: boolean;
  /** Makes repeated requests for the same machine distinct. */
  at: number;
}

interface Props {
  view: MachinesView;
  focus: Focus | null;
  onError: (e: string) => void;
}

export function Compare({ view, focus, onError }: Props) {
  const [selected, setSelected] = useState<string | null>(focus?.id ?? view.machines[0]?.id ?? null);
  const [data, setData] = useState<MachineComparison | null>(null);
  const [plan, setPlan] = useState<Step[] | null>(null);
  const [row, setRow] = useState<ComparisonRow | null>(null);
  const [wantPlan, setWantPlan] = useState(focus?.plan ?? false);

  useEffect(() => {
    if (!focus) return;
    setSelected(focus.id);
    setWantPlan(focus.plan);
  }, [focus]);

  useEffect(() => {
    if (!wantPlan || !data || data.machine.id !== selected) return;
    setWantPlan(false);
    planMachine(data.machine.id).then((p) => setPlan(p.steps), (e) => onError(String(e)));
  }, [wantPlan, data, selected, onError]);

  useEffect(() => {
    if (!view.machines.some((m) => m.id === selected)) setSelected(view.machines[0]?.id ?? null);
  }, [view, selected]);

  useEffect(() => {
    setData(null);
    setPlan(null);
    setRow(null);
    if (!selected || view.golden_version === null) return;
    compareMachine(selected).then(setData, (e) => onError(String(e)));
  }, [selected, view, onError]);

  return (
    <div className="flex h-full min-h-0">
      <aside className="flex w-52 shrink-0 flex-col border-r border-line bg-panel">
        <div className="px-4 pb-1.5 pt-3 text-[11px] text-dim">Machines</div>
        <ul className="flex flex-1 flex-col gap-0.5 overflow-auto px-2">
          {view.machines.map((m) => (
            <MachineItem key={m.id} m={m} active={m.id === selected} onClick={() => setSelected(m.id)} />
          ))}
        </ul>
        <p className="p-4 text-[11px] leading-relaxed text-dim">
          Your other machines show up here once they're linked. Use Link a machine in the top bar.
        </p>
      </aside>

      <section className="min-w-0 flex-1 overflow-auto">
        {view.golden_version === null ? (
          <Empty text="Make a Golden Rig first. Scan your main machine and choose Make Golden." />
        ) : !data ? (
          <Empty text={selected ? "Comparing…" : "No machines yet."} />
        ) : plan ? (
          <PlanView
            name={data.machine.name}
            here={data.machine.is_this_machine}
            steps={plan}
            onBack={() => setPlan(null)}
            onError={onError}
          />
        ) : (
          <Diff
            data={data}
            selected={row}
            onSelect={setRow}
            onPlan={() => planMachine(data.machine.id).then((p) => setPlan(p.steps), (e) => onError(String(e)))}
          />
        )}
      </section>

      <aside className="w-64 shrink-0 overflow-auto border-l border-line bg-panel">
        {row && data ? <Detail row={row} machine={data.machine} /> : (
          <p className="p-4 text-xs text-dim">Select a row to see its versions here and on the Golden Rig.</p>
        )}
      </aside>
    </div>
  );
}

function MachineItem({ m, active, onClick }: { m: MachineCard; active: boolean; onClick: () => void }) {
  return (
    <li>
      <button
        onClick={onClick}
        className={`flex w-full flex-col gap-1.5 rounded-md px-2.5 py-2 text-left ${
          active ? "bg-sel" : "hover:bg-sel/50"
        }`}
      >
        <span className="flex w-full items-baseline gap-2">
          <span className="min-w-0 flex-1 truncate font-medium">{m.name}</span>
          <span className={`font-semibold tabular-nums ${scoreColor(m.score)}`}>{m.score ?? "–"}</span>
        </span>
        <ScoreBar score={m.score} className="w-full" />
        <span className="block w-full truncate text-[11px] text-dim">
          {m.os === "macos" ? "macOS" : "Windows"}
          {m.is_this_machine ? " · this machine" : ""}
          {m.profile ? ` · ${m.profile.name}` : ""}
        </span>
      </button>
    </li>
  );
}

function Empty({ text }: { text: string }) {
  return <p className="p-6 text-dim">{text}</p>;
}

function Diff({
  data,
  selected,
  onSelect,
  onPlan,
}: {
  data: MachineComparison;
  selected: ComparisonRow | null;
  onSelect: (r: ComparisonRow) => void;
  onPlan: () => void;
}) {
  const { machine, comparison: c } = data;
  const [states, setStates] = useState<Set<ParityState>>(new Set());
  const [vendor, setVendor] = useState("");
  const [format, setFormat] = useState<PluginFormat | "">("");
  const [showParity, setShowParity] = useState(false);

  const counts = useMemo(() => {
    const n = new Map<ParityState, number>();
    for (const r of c.rows) n.set(r.state, (n.get(r.state) ?? 0) + 1);
    return n;
  }, [c]);
  const vendors = useMemo(() => [...new Set(c.rows.map((r) => r.vendor).filter(Boolean))].sort(), [c]);
  const formats = useMemo(
    () => [...new Set(c.rows.flatMap((r) => [...r.golden_items, ...r.machine_items].map((i) => i.format)))]
      .filter((f): f is PluginFormat => f !== null),
    [c],
  );

  const rows = c.rows.filter(
    (r) =>
      (states.size ? states.has(r.state) : showParity || r.state !== "in_parity") &&
      (!vendor || r.vendor === vendor) &&
      (!format || [...r.golden_items, ...r.machine_items].some((i) => i.format === format)),
  );
  const toggle = (s: ParityState) =>
    setStates((prev) => {
      const next = new Set(prev);
      if (next.has(s)) next.delete(s);
      else next.add(s);
      return next;
    });
  const hasWork = c.rows.some((r) => ["missing", "incomplete", "outdated"].includes(r.state));

  return (
    <div className="flex flex-col gap-3 px-5 py-4">
      <header className="flex items-center gap-4">
        <span className={`text-3xl font-semibold leading-none tracking-tight tabular-nums ${scoreColor(c.score)}`}>
          {c.score}
        </span>
        <div className="min-w-0 flex-1">
          <h1 className="truncate text-sm font-semibold">{machine.name}</h1>
          <p className="truncate text-xs text-dim">
            compared with Golden Rig v{data.golden_version} · {machine.profile ? `${machine.profile.name} profile` : "no profile"} ·
            scanned {timeAgo(machine.scanned_at)}
          </p>
        </div>
        {hasWork && <Button onClick={onPlan}>Up To Speed</Button>}
      </header>

      {c.reasons.length > 0 ? (
        <div className="flex flex-col gap-1.5">
          <div className="flex h-1.5 overflow-hidden rounded-full bg-track" aria-hidden="true">
            <span className="bg-ok" style={{ width: `${c.score}%` }} />
            {c.reasons.map((r, i) => (
              <span
                key={r.text}
                className="bg-bad"
                style={{ width: `${r.points}%`, opacity: Math.max(1 - i * 0.15, 0.35) }}
              />
            ))}
          </div>
          <ul className="flex flex-wrap gap-x-3 gap-y-0.5 text-xs text-dim">
            {c.reasons.map((r) => (
              <li key={r.text}>
                <span className="tabular-nums text-fg">−{Math.round(r.points)}</span> {r.text}
              </li>
            ))}
          </ul>
        </div>
      ) : (
        <p className="text-xs text-ok">At parity with the Golden Rig.</p>
      )}

      {c.templates.some((t) => t.missing.length > 0) && (
        <div className="rounded-md bg-warn/10 px-3 py-2 text-xs">
          <span className="font-medium text-warn">Templates not ready: </span>
          <span className="text-dim">
            {c.templates
              .filter((t) => t.missing.length > 0)
              .map((t) => `${t.name} needs ${t.missing.length} plug-in${t.missing.length === 1 ? "" : "s"}`)
              .join(" · ")}
          </span>
        </div>
      )}

      <div className="flex flex-wrap items-center gap-1 border-b border-line pb-2">
        {STATE_ORDER.filter((s) => counts.get(s)).map((s) => (
          <Pill key={s} active={states.has(s)} onClick={() => toggle(s)}>
            {STATE_LABEL[s]} <span className="tabular-nums opacity-70">{counts.get(s)}</span>
          </Pill>
        ))}
        <span className="flex-1" />
        {!states.size && (
          <label className="mr-2 flex items-center gap-1.5 text-xs text-dim">
            <input type="checkbox" checked={showParity} onChange={(e) => setShowParity(e.target.checked)} />
            Show in parity
          </label>
        )}
        <select
          value={vendor}
          onChange={(e) => setVendor(e.target.value)}
          aria-label="Vendor"
          className={`h-6 max-w-36 ${inputClass}`}
        >
          <option value="">All vendors</option>
          {vendors.map((v) => (
            <option key={v}>{v}</option>
          ))}
        </select>
        <select
          value={format}
          onChange={(e) => setFormat(e.target.value as PluginFormat | "")}
          aria-label="Format"
          className={`h-6 ${inputClass}`}
        >
          <option value="">All formats</option>
          {formats.map((f) => (
            <option key={f} value={f}>
              {FORMAT_LABEL[f]}
            </option>
          ))}
        </select>
      </div>

      <div className="-mx-2 flex flex-col gap-2">
        {STATE_ORDER.map((s) => {
          const group = rows.filter((r) => r.state === s);
          if (!group.length) return null;
          return (
            <div key={s}>
              <h3 className="px-2 pb-0.5 text-[11px] text-dim">
                {STATE_LABEL[s]} <span className="tabular-nums opacity-70">{group.length}</span>
              </h3>
              <ul>
                {group.map((r) => (
                  <li key={r.product_id}>
                    <button
                      onClick={() => onSelect(r)}
                      className={`flex h-7 w-full items-center gap-2.5 rounded-[5px] px-2 text-left ${
                        selected?.product_id === r.product_id ? "bg-sel" : "hover:bg-sel/50"
                      }`}
                    >
                      <span className={`h-1.5 w-1.5 shrink-0 rounded-full ${STATE_DOT[r.state]}`} aria-hidden="true" />
                      <span className="min-w-0 flex-1 truncate">
                        {r.name}
                        {r.vendor && <span className="ml-2 text-xs text-dim">{r.vendor}</span>}
                      </span>
                      {r.needed_by_templates && <Badge tone="amber">template</Badge>}
                      {r.confidence === "low" && <Badge tone="amber">name only</Badge>}
                      {r.missing_formats.map((f) => (
                        <Badge key={f}>no {FORMAT_LABEL[f]}</Badge>
                      ))}
                      <span className="w-28 shrink-0 text-right font-mono text-[11px] text-dim">{versionPair(r)}</span>
                    </button>
                  </li>
                ))}
              </ul>
            </div>
          );
        })}
        {!rows.length && (states.size > 0 || vendor || format) && (
          <p className="px-2 text-dim">Nothing to show with these filters.</p>
        )}
        {c.unresolved > 0 && (
          <p className="px-2 text-[11px] text-dim">
            {c.unresolved} file{c.unresolved === 1 ? "" : "s"} on this machine couldn't be identified and aren't
            compared.
          </p>
        )}
      </div>
    </div>
  );
}

function versionPair(r: ComparisonRow): string {
  const g = r.golden_version ?? "";
  const m = r.machine_version ?? "";
  if (r.state === "missing") return g;
  if (r.state === "extra" || g === m) return m;
  return `${m || "?"} → ${g || "?"}`;
}

function Detail({ row, machine }: { row: ComparisonRow; machine: MachineCard }) {
  return (
    <div className="flex flex-col gap-4 p-4">
      <div>
        <div className="text-sm font-semibold">{row.name}</div>
        <div className="text-xs text-dim">{row.vendor || "Unknown vendor"}</div>
      </div>
      <p className="text-xs leading-relaxed">
        <span className="mr-1.5 inline-flex items-center gap-1.5 font-medium">
          <span className={`h-1.5 w-1.5 rounded-full ${STATE_DOT[row.state]}`} aria-hidden="true" />
          {STATE_LABEL[row.state]}.
        </span>
        <span className="text-dim">{STATE_HINT[row.state]}</span>
      </p>
      <Side title="Golden Rig" items={row.golden_items} />
      <Side title={machine.name} items={row.machine_items} />
      {row.needed_by_templates && (
        <p className="text-xs text-dim">A template uses this plug-in, so it counts three times in the score.</p>
      )}
      {row.confidence === "low" && (
        <p className="text-xs text-dim">Identified from the file name only, so the match may be wrong.</p>
      )}
    </div>
  );
}

function Side({ title, items }: { title: string; items: ComparisonRow["golden_items"] }) {
  return (
    <div>
      <div className="mb-1 text-[11px] text-dim">{title}</div>
      {items.length === 0 ? (
        <p className="text-xs text-bad">Not installed</p>
      ) : (
        <ul className="flex flex-col gap-1">
          {items.map((i, n) => (
            <li key={n} className="flex items-center justify-between text-xs">
              <span>
                {i.format ? FORMAT_LABEL[i.format] : "—"}{" "}
                <span className="text-dim">{i.platform === "macos" ? "macOS" : "Windows"}</span>
              </span>
              <span className="font-mono text-[11px] text-dim">{i.version ?? "?"}</span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function PlanView({
  name,
  here,
  steps,
  onBack,
  onError,
}: {
  name: string;
  here: boolean;
  steps: Step[];
  onBack: () => void;
  onError: (e: string) => void;
}) {
  const [done, setDone] = useState<Set<number>>(new Set());
  return (
    <div className="flex flex-col gap-3 px-5 py-4">
      <header className="flex items-center gap-4">
        <div className="min-w-0 flex-1">
          <h1 className="truncate text-sm font-semibold">Up To Speed: {name}</h1>
          <p className="text-xs text-dim">
            {here
              ? "A checklist. Templates, scripts and themes can be brought over below; install plug-ins yourself, then rescan."
              : `A checklist. Open Patch Pilot on ${name} to bring templates, scripts and themes over there.`}
          </p>
        </div>
        <span className="text-xs tabular-nums text-dim">
          {done.size} of {steps.length}
        </span>
        <Button variant="quiet" onClick={onBack} className="shrink-0 whitespace-nowrap">
          Back to diff
        </Button>
      </header>
      {here && <BringOver onError={onError} />}
      {steps.length === 0 && <p className="text-dim">Nothing to do.</p>}
      <ol className="-mx-2 flex flex-col">
        {steps.map((s, i) => (
          <li key={s.number}>
            {(i === 0 || steps[i - 1].phase !== s.phase) && (
              <h3 className="px-2 pb-0.5 pt-3 text-[11px] text-dim">{PHASE_LABEL[s.phase]}</h3>
            )}
            <label className="flex gap-2.5 rounded-[5px] px-2 py-1.5 hover:bg-sel/50">
              <input
                type="checkbox"
                className="mt-0.5 accent-accent"
                checked={done.has(s.number)}
                onChange={() =>
                  setDone((d) => {
                    const n = new Set(d);
                    if (n.has(s.number)) n.delete(s.number);
                    else n.add(s.number);
                    return n;
                  })
                }
              />
              <span className="min-w-0 flex-1">
                <span className={done.has(s.number) ? "text-dim line-through" : ""}>
                  {s.number}. {s.title}
                </span>
                {s.details.length > 0 && (
                  <ul className="mt-0.5 text-xs text-dim">
                    {s.details.map((d) => (
                      <li key={d}>{d}</li>
                    ))}
                  </ul>
                )}
                {s.after.length > 0 && (
                  <span className="mt-0.5 block text-[11px] text-dim opacity-80">
                    After step{s.after.length === 1 ? "" : "s"} {s.after.join(", ")}
                  </span>
                )}
              </span>
            </label>
          </li>
        ))}
      </ol>
    </div>
  );
}
