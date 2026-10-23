import { useMemo, useState } from "react";
import { FORMAT_LABEL, type Overview, type PluginFormat, type ProductRow } from "../api";
import { Badge, Button, inputClass, Pill, timeAgo } from "../ui";

type Grouping = "vendor" | "format";

interface Props {
  overview: Overview;
  warnings: string[];
  busy: boolean;
  onRescan: () => void;
  onMakeGolden: () => void;
}

export function Rig({ overview, warnings, busy, onRescan, onMakeGolden }: Props) {
  const { machine, inventory: inv, golden } = overview;
  const [grouping, setGrouping] = useState<Grouping>("vendor");
  const [query, setQuery] = useState("");

  const groups = useMemo(() => {
    if (!inv) return [];
    const q = query.trim().toLowerCase();
    const match = (vendor: string, p: ProductRow) =>
      !q || p.name.toLowerCase().includes(q) || vendor.toLowerCase().includes(q);
    if (grouping === "vendor") {
      return inv.vendors
        .map((v) => ({
          title: v.vendor || "Unknown vendor",
          rows: v.products.filter((p) => match(v.vendor, p)).map((p) => ({ vendor: v.vendor, p })),
        }))
        .filter((g) => g.rows.length > 0);
    }
    return inv.formats
      .map(({ format }) => ({
        title: FORMAT_LABEL[format],
        rows: inv.vendors.flatMap((v) =>
          v.products
            .filter((p) => p.formats.includes(format) && match(v.vendor, p))
            .map((p) => ({ vendor: v.vendor, p })),
        ),
      }))
      .filter((g) => g.rows.length > 0);
  }, [inv, grouping, query]);

  if (!machine || !inv) return null;

  const vendorCount = inv.vendors.filter((v) => v.vendor).length;
  const plural = (n: number, word: string) => `${n.toLocaleString()} ${word}${n === 1 ? "" : "s"}`;

  return (
    <div className="flex flex-col pb-6">
      <header className="flex items-center gap-3 border-b border-line px-4 py-2.5">
        <div className="min-w-0 flex-1">
          <h1 className="truncate text-sm font-semibold">{machine.name}</h1>
          <p className="truncate text-xs text-dim">
            {machine.os === "macos" ? "macOS" : "Windows"} · scanned {timeAgo(machine.scanned_at)} ·{" "}
            {plural(inv.products, "plug-in")} ({plural(inv.plugins, "file")}) · {plural(vendorCount, "vendor")} ·{" "}
            {plural(inv.templates, "template")}
          </p>
        </div>
        <Button variant="quiet" onClick={onRescan} disabled={busy}>
          Rescan
        </Button>
      </header>

      <GoldenCard golden={golden} busy={busy} onMakeGolden={onMakeGolden} />

      {warnings.length > 0 && (
        <details className="border-b border-line px-4 py-2 text-xs">
          <summary className="cursor-pointer text-warn">
            {warnings.length} file{warnings.length === 1 ? "" : "s"} could only be read partly
          </summary>
          <ul className="mt-1.5 max-h-40 overflow-auto font-mono text-[11px] text-dim">
            {warnings.map((w) => (
              <li key={w}>{w}</li>
            ))}
          </ul>
        </details>
      )}

      <div className="flex flex-wrap items-center gap-1 px-3 pb-1 pt-3">
        {(["vendor", "format"] as const).map((g) => (
          <Pill key={g} active={grouping === g} onClick={() => setGrouping(g)}>
            By {g}
          </Pill>
        ))}
        <span className="ml-2 flex flex-wrap gap-2 text-xs text-dim">
          {inv.formats.map((f) => (
            <span key={f.format}>
              {FORMAT_LABEL[f.format]} <span className="tabular-nums">{f.count}</span>
            </span>
          ))}
        </span>
        <input
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="Filter"
          aria-label="Filter plug-ins"
          className={`ml-auto w-40 ${inputClass}`}
        />
      </div>

      {groups.length === 0 && (
        <p className="px-4 py-3 text-dim">
          {inv.products === 0 ? "No plug-ins found in the standard folders." : "Nothing matches."}
        </p>
      )}
      {groups.map((g) => (
        <section key={g.title}>
          <h3 className="flex items-baseline gap-1.5 px-4 pb-0.5 pt-2.5 text-[11px] text-dim">
            {g.title}
            <span className="tabular-nums opacity-70">{g.rows.length}</span>
          </h3>
          <ul>
            {g.rows.map(({ vendor, p }) => (
              <ProductLine key={`${g.title}/${p.id}`} p={p} vendor={grouping === "format" ? vendor : null} />
            ))}
          </ul>
        </section>
      ))}

      {inv.apps.length > 0 && (
        <section>
          <h3 className="px-4 pb-0.5 pt-4 text-[11px] text-dim">Vendor apps and licensing</h3>
          <ul>
            {inv.apps.map((a) => (
              <li key={a.name} className="flex h-7 items-center justify-between px-4 hover:bg-sel/60">
                <span>{a.name}</span>
                <span className="font-mono text-[11px] text-dim">{a.version ?? ""}</span>
              </li>
            ))}
          </ul>
        </section>
      )}
    </div>
  );
}

function ProductLine({ p, vendor }: { p: ProductRow; vendor: string | null }) {
  return (
    <li className="flex h-7 items-center gap-3 px-4 hover:bg-sel/60">
      <span className="min-w-0 flex-1 truncate">
        {p.name}
        {vendor && <span className="ml-2 text-xs text-dim">{vendor}</span>}
      </span>
      {p.confidence === "low" && (
        <span title="Identified from the file name only">
          <Badge tone="amber">name only</Badge>
        </span>
      )}
      <span className="text-xs text-dim">{p.formats.map((f: PluginFormat) => FORMAT_LABEL[f]).join(" · ")}</span>
      <span className="w-16 text-right font-mono text-[11px] text-dim">{p.version ?? "?"}</span>
    </li>
  );
}

function GoldenCard({
  golden,
  busy,
  onMakeGolden,
}: {
  golden: Overview["golden"];
  busy: boolean;
  onMakeGolden: () => void;
}) {
  if (golden) {
    return (
      <p className="flex items-center gap-2 border-b border-line px-4 py-2 text-xs text-dim">
        <span className="h-1.5 w-1.5 shrink-0 rounded-full bg-ok" aria-hidden="true" />
        <span>
          <span className="font-medium text-fg">Golden Rig v{golden.version}</span>
          {golden.from_this_machine
            ? " was made from this machine."
            : " comes from another machine. Open Compare to see how this one measures up."}
        </span>
      </p>
    );
  }
  return (
    <section className="flex items-center gap-3 border-b border-line px-4 py-2 text-xs">
      <span className="min-w-0 flex-1 text-dim">
        <span className="font-medium text-fg">Make this your Golden Rig?</span> Your other machines will be
        compared against this setup. You can change it later.
      </span>
      <Button onClick={onMakeGolden} disabled={busy}>
        Make Golden
      </Button>
    </section>
  );
}
