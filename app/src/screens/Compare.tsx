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
import { Badge, Button, timeAgo } from "../ui";

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

export function scoreTone(score: number | null): string {
  if (score === null) return "bg-neutral-100 text-neutral-500 dark:bg-neutral-800";
  if (score >= 95) return "bg-emerald-100 text-emerald-800 dark:bg-emerald-900/40 dark:text-emerald-300";
  if (score >= 75) return "bg-amber-100 text-amber-800 dark:bg-amber-900/40 dark:text-amber-300";
  return "bg-red-100 text-red-800 dark:bg-red-900/40 dark:text-red-300";
}

interface Props {
  view: MachinesView;
  onError: (e: string) => void;
}

export function Compare({ view, onError }: Props) {
  const [selected, setSelected] = useState<string | null>(view.machines[0]?.id ?? null);
  const [data, setData] = useState<MachineComparison | null>(null);
  const [plan, setPlan] = useState<Step[] | null>(null);
  const [row, setRow] = useState<ComparisonRow | null>(null);

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
    <div className="flex h-[calc(100vh-3rem)] min-h-0">
      <aside className="flex w-60 shrink-0 flex-col border-r border-neutral-200 dark:border-neutral-800">
        <div className="px-4 pb-2 pt-4 text-xs font-medium uppercase tracking-wide text-neutral-500">
          Machines
        </div>
        <ul className="flex-1 overflow-auto px-2">
          {view.machines.map((m) => (
            <MachineItem key={m.id} m={m} active={m.id === selected} onClick={() => setSelected(m.id)} />
          ))}
        </ul>
        <p className="border-t border-neutral-200 p-4 text-xs text-neutral-500 dark:border-neutral-800">
          Add a machine: on it, run <code className="font-mono">patchpilot export</code>, then drop the
          file on this window.
        </p>
      </aside>

      <section className="min-w-0 flex-1 overflow-auto">
        {view.golden_version === null ? (
          <Empty text="Make a Golden Rig first. Scan your main machine and choose Make Golden." />
        ) : !data ? (
          <Empty text={selected ? "Comparing…" : "No machines yet."} />
        ) : plan ? (
          <PlanView name={data.machine.name} steps={plan} onBack={() => setPlan(null)} />
        ) : (
          <Diff
            data={data}
            selected={row}
            onSelect={setRow}
            onPlan={() => planMachine(data.machine.id).then((p) => setPlan(p.steps), (e) => onError(String(e)))}
          />
        )}
      </section>

      <aside className="w-72 shrink-0 overflow-auto border-l border-neutral-200 dark:border-neutral-800">
        {row && data ? <Detail row={row} machine={data.machine} /> : (
          <p className="p-4 text-xs text-neutral-500">Select a product to see versions on both sides.</p>
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
        className={`mb-1 flex w-full items-center gap-3 rounded-md px-2 py-2 text-left ${
          active ? "bg-neutral-100 dark:bg-neutral-800" : "hover:bg-neutral-50 dark:hover:bg-neutral-900"
        }`}
      >
        <span className="min-w-0 flex-1">
          <span className="block truncate text-sm font-medium">{m.name}</span>
          <span className="block truncate text-xs text-neutral-500">
            {m.os === "macos" ? "macOS" : "Windows"}
            {m.is_this_machine ? " · this machine" : ""}
            {m.profile ? ` · ${m.profile.name}` : ""}
          </span>
        </span>
        <span className={`rounded px-1.5 py-0.5 text-xs font-semibold tabular-nums ${scoreTone(m.score)}`}>
          {m.score ?? "–"}
        </span>
      </button>
    </li>
  );
}

function Empty({ text }: { text: string }) {
  return <p className="p-8 text-sm text-neutral-500">{text}</p>;
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
    <div className="flex flex-col gap-6 p-6">
      <header className="flex items-start gap-4">
        <div className={`rounded-lg px-3 py-2 text-3xl font-semibold tabular-nums ${scoreTone(c.score)}`}>
          {c.score}
        </div>
        <div className="min-w-0 flex-1">
          <h1 className="text-xl font-semibold tracking-tight">{machine.name}</h1>
          <p className="text-xs text-neutral-500">
            vs Golden Rig v{data.golden_version} · {machine.profile ? `${machine.profile.name} profile` : "no profile"} ·
            scanned {timeAgo(machine.scanned_at)}
          </p>
        </div>
        {hasWork && <Button onClick={onPlan}>Up To Speed</Button>}
      </header>

      {c.reasons.length > 0 ? (
        <ul className="flex flex-col gap-1 text-sm">
          {c.reasons.map((r) => (
            <li key={r.text} className="flex gap-3">
              <span className="w-12 shrink-0 text-right font-mono text-xs leading-5 text-red-600 dark:text-red-400">
                −{Math.round(r.points)}
              </span>
              <span>{r.text}</span>
            </li>
          ))}
        </ul>
      ) : (
        <p className="text-sm text-emerald-700 dark:text-emerald-400">At parity with the Golden Rig.</p>
      )}

      {c.templates.some((t) => t.missing.length > 0) && (
        <div className="rounded-lg border border-amber-200 px-4 py-3 text-sm dark:border-amber-900/60">
          <div className="font-medium">Templates not ready</div>
          <ul className="mt-1 text-xs text-neutral-600 dark:text-neutral-400">
            {c.templates
              .filter((t) => t.missing.length > 0)
              .map((t) => (
                <li key={t.product_id}>
                  {t.name}: needs {t.missing.length} plug-in{t.missing.length === 1 ? "" : "s"}
                </li>
              ))}
          </ul>
        </div>
      )}

      <div className="flex flex-wrap items-center gap-2 text-xs">
        {STATE_ORDER.filter((s) => counts.get(s)).map((s) => (
          <button
            key={s}
            onClick={() => toggle(s)}
            className={`rounded-full border px-2.5 py-1 ${
              states.has(s)
                ? "border-neutral-900 bg-neutral-900 text-white dark:border-white dark:bg-white dark:text-neutral-900"
                : "border-neutral-300 text-neutral-600 dark:border-neutral-700 dark:text-neutral-300"
            }`}
          >
            {STATE_LABEL[s]} <span className="tabular-nums">{counts.get(s)}</span>
          </button>
        ))}
        <span className="mx-1 h-4 w-px bg-neutral-200 dark:bg-neutral-800" />
        <select
          value={vendor}
          onChange={(e) => setVendor(e.target.value)}
          className="rounded-md border border-neutral-300 bg-transparent px-2 py-1 dark:border-neutral-700"
        >
          <option value="">All vendors</option>
          {vendors.map((v) => (
            <option key={v}>{v}</option>
          ))}
        </select>
        <select
          value={format}
          onChange={(e) => setFormat(e.target.value as PluginFormat | "")}
          className="rounded-md border border-neutral-300 bg-transparent px-2 py-1 dark:border-neutral-700"
        >
          <option value="">All formats</option>
          {formats.map((f) => (
            <option key={f} value={f}>
              {FORMAT_LABEL[f]}
            </option>
          ))}
        </select>
        {!states.size && (
          <label className="ml-auto flex items-center gap-1.5 text-neutral-500">
            <input type="checkbox" checked={showParity} onChange={(e) => setShowParity(e.target.checked)} />
            Show in parity
          </label>
        )}
      </div>

      <div className="flex flex-col gap-4">
        {STATE_ORDER.map((s) => {
          const group = rows.filter((r) => r.state === s);
          if (!group.length) return null;
          return (
            <div key={s}>
              <h3 className="mb-1 text-xs font-medium text-neutral-500">
                {STATE_LABEL[s]} <span className="tabular-nums text-neutral-400">{group.length}</span>
              </h3>
              <ul className="divide-y divide-neutral-100 rounded-lg border border-neutral-200 dark:divide-neutral-900 dark:border-neutral-800">
                {group.map((r) => (
                  <li key={r.product_id}>
                    <button
                      onClick={() => onSelect(r)}
                      className={`flex w-full items-center gap-3 px-3 py-2 text-left text-sm ${
                        selected?.product_id === r.product_id ? "bg-neutral-50 dark:bg-neutral-900" : ""
                      }`}
                    >
                      <span className="min-w-0 flex-1 truncate">
                        {r.name}
                        {r.vendor && <span className="ml-2 text-xs text-neutral-500">{r.vendor}</span>}
                      </span>
                      {r.needed_by_templates && <Badge tone="amber">template</Badge>}
                      {r.confidence === "low" && <Badge tone="amber">name only</Badge>}
                      {r.missing_formats.map((f) => (
                        <Badge key={f}>no {FORMAT_LABEL[f]}</Badge>
                      ))}
                      <span className="w-32 text-right font-mono text-xs text-neutral-500">
                        {versionPair(r)}
                      </span>
                    </button>
                  </li>
                ))}
              </ul>
            </div>
          );
        })}
        {!rows.length && (states.size > 0 || vendor || format) && (
          <p className="text-sm text-neutral-500">Nothing to show with these filters.</p>
        )}
        {c.unresolved > 0 && (
          <p className="text-xs text-neutral-500">
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
    <div className="flex flex-col gap-4 p-4 text-sm">
      <div>
        <div className="font-semibold">{row.name}</div>
        <div className="text-xs text-neutral-500">{row.vendor || "Unknown vendor"}</div>
      </div>
      <div>
        <Badge>{STATE_LABEL[row.state]}</Badge>
        <p className="mt-1 text-xs text-neutral-600 dark:text-neutral-400">{STATE_HINT[row.state]}</p>
      </div>
      <Side title="Golden Rig" items={row.golden_items} />
      <Side title={machine.name} items={row.machine_items} />
      {row.needed_by_templates && (
        <p className="text-xs text-neutral-600 dark:text-neutral-400">
          A template uses this plug-in, so it counts three times in the score.
        </p>
      )}
      {row.confidence === "low" && (
        <p className="text-xs text-neutral-600 dark:text-neutral-400">
          Identified from the file name only, so the match may be wrong.
        </p>
      )}
    </div>
  );
}

function Side({ title, items }: { title: string; items: ComparisonRow["golden_items"] }) {
  return (
    <div>
      <div className="mb-1 text-xs font-medium text-neutral-500">{title}</div>
      {items.length === 0 ? (
        <p className="text-xs text-neutral-400">Not installed</p>
      ) : (
        <ul className="flex flex-col gap-1">
          {items.map((i, n) => (
            <li key={n} className="flex items-center justify-between text-xs">
              <span>
                {i.format ? FORMAT_LABEL[i.format] : "—"}{" "}
                <span className="text-neutral-500">{i.platform === "macos" ? "macOS" : "Windows"}</span>
              </span>
              <span className="font-mono text-neutral-500">{i.version ?? "?"}</span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function PlanView({ name, steps, onBack }: { name: string; steps: Step[]; onBack: () => void }) {
  const [done, setDone] = useState<Set<number>>(new Set());
  return (
    <div className="flex flex-col gap-5 p-6">
      <header className="flex items-center justify-between">
        <div>
          <h1 className="text-xl font-semibold tracking-tight">Up To Speed: {name}</h1>
          <p className="text-xs text-neutral-500">
            A checklist. Patch Pilot doesn't install or copy anything; rescan when you're done.
          </p>
        </div>
        <Button variant="quiet" onClick={onBack} className="shrink-0 whitespace-nowrap">
          Back to diff
        </Button>
      </header>
      {steps.length === 0 && <p className="text-sm text-neutral-500">Nothing to do.</p>}
      <ol className="flex flex-col gap-3">
        {steps.map((s, i) => (
          <li key={s.number}>
            {(i === 0 || steps[i - 1].phase !== s.phase) && (
              <h3 className="mb-2 mt-2 text-xs font-medium uppercase tracking-wide text-neutral-500">
                {PHASE_LABEL[s.phase]}
              </h3>
            )}
            <label className="flex gap-3 rounded-lg border border-neutral-200 px-3 py-2 dark:border-neutral-800">
              <input
                type="checkbox"
                className="mt-1"
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
                <span className={`text-sm ${done.has(s.number) ? "text-neutral-400 line-through" : ""}`}>
                  {s.number}. {s.title}
                </span>
                {s.details.length > 0 && (
                  <ul className="mt-1 text-xs text-neutral-500">
                    {s.details.map((d) => (
                      <li key={d}>{d}</li>
                    ))}
                  </ul>
                )}
                {s.after.length > 0 && (
                  <span className="mt-1 block text-[11px] text-neutral-400">
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
