import { useMemo, useState } from "react";
import { FORMAT_LABEL, type Overview, type PluginFormat, type ProductRow } from "../api";
import { Badge, Button, Stat, timeAgo } from "../ui";

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

  return (
    <div className="mx-auto flex max-w-3xl flex-col gap-8 px-6 py-10">
      <header className="flex items-start justify-between gap-4">
        <div>
          <p className="text-xs font-medium uppercase tracking-wide text-neutral-500">
            Here's your rig
          </p>
          <h1 className="mt-1 text-2xl font-semibold tracking-tight">{machine.name}</h1>
          <p className="mt-1 text-xs text-neutral-500">
            {machine.os === "macos" ? "macOS" : "Windows"} · scanned {timeAgo(machine.scanned_at)}
          </p>
        </div>
        <Button variant="quiet" onClick={onRescan} disabled={busy}>
          Rescan
        </Button>
      </header>

      <section className="grid grid-cols-2 gap-3 sm:grid-cols-4">
        <Stat label="Plug-ins" value={inv.products} />
        <Stat label="Plug-in files" value={inv.plugins} />
        <Stat label="Vendors" value={inv.vendors.filter((v) => v.vendor).length} />
        <Stat label="Templates" value={inv.templates} />
      </section>

      <GoldenCard golden={golden} busy={busy} onMakeGolden={onMakeGolden} />

      {warnings.length > 0 && (
        <details className="rounded-lg border border-amber-200 px-4 py-3 text-sm dark:border-amber-900/60">
          <summary className="cursor-pointer text-amber-800 dark:text-amber-300">
            {warnings.length} file{warnings.length === 1 ? "" : "s"} could only be read partly
          </summary>
          <ul className="mt-2 max-h-48 overflow-auto font-mono text-xs text-neutral-500">
            {warnings.map((w) => (
              <li key={w}>{w}</li>
            ))}
          </ul>
        </details>
      )}

      <section className="flex flex-col gap-4">
        <div className="flex flex-wrap items-center gap-3">
          <h2 className="mr-auto text-sm font-semibold">Plug-ins</h2>
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Filter"
            className="w-40 rounded-md border border-neutral-300 bg-transparent px-2.5 py-1.5 text-sm outline-none focus:border-neutral-900 dark:border-neutral-700 dark:focus:border-neutral-300"
          />
          <div className="flex rounded-md border border-neutral-300 p-0.5 text-xs dark:border-neutral-700">
            {(["vendor", "format"] as const).map((g) => (
              <button
                key={g}
                onClick={() => setGrouping(g)}
                className={`rounded px-2.5 py-1 ${
                  grouping === g
                    ? "bg-neutral-900 text-white dark:bg-white dark:text-neutral-900"
                    : "text-neutral-500"
                }`}
              >
                By {g}
              </button>
            ))}
          </div>
        </div>
        <p className="flex flex-wrap gap-2 text-xs text-neutral-500">
          {inv.formats.map((f) => (
            <span key={f.format}>
              {FORMAT_LABEL[f.format]} <span className="tabular-nums">{f.count}</span>
            </span>
          ))}
        </p>
        {groups.length === 0 && (
          <p className="text-sm text-neutral-500">
            {inv.products === 0 ? "No plug-ins found in the standard folders." : "Nothing matches."}
          </p>
        )}
        {groups.map((g) => (
          <div key={g.title}>
            <h3 className="mb-1 flex items-baseline gap-2 text-xs font-medium text-neutral-500">
              {g.title}
              <span className="tabular-nums text-neutral-400">{g.rows.length}</span>
            </h3>
            <ul className="divide-y divide-neutral-100 rounded-lg border border-neutral-200 dark:divide-neutral-900 dark:border-neutral-800">
              {g.rows.map(({ vendor, p }) => (
                <ProductLine key={`${g.title}/${p.id}`} p={p} vendor={grouping === "format" ? vendor : null} />
              ))}
            </ul>
          </div>
        ))}
      </section>

      {inv.apps.length > 0 && (
        <section>
          <h2 className="mb-2 text-sm font-semibold">Vendor apps and licensing</h2>
          <ul className="divide-y divide-neutral-100 rounded-lg border border-neutral-200 text-sm dark:divide-neutral-900 dark:border-neutral-800">
            {inv.apps.map((a) => (
              <li key={a.name} className="flex items-center justify-between px-3 py-2">
                <span>{a.name}</span>
                <span className="font-mono text-xs text-neutral-500">{a.version ?? ""}</span>
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
    <li className="flex items-center gap-3 px-3 py-2 text-sm">
      <span className="min-w-0 flex-1 truncate">
        {p.name}
        {vendor && <span className="ml-2 text-xs text-neutral-500">{vendor}</span>}
      </span>
      {p.confidence === "low" && (
        <span title="Identified from the file name only">
          <Badge tone="amber">name only</Badge>
        </span>
      )}
      <span className="flex gap-1">
        {p.formats.map((f: PluginFormat) => (
          <Badge key={f}>{FORMAT_LABEL[f]}</Badge>
        ))}
      </span>
      <span className="w-20 text-right font-mono text-xs text-neutral-500">{p.version ?? "?"}</span>
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
      <section className="rounded-lg border border-emerald-200 bg-emerald-50 px-4 py-3 text-sm dark:border-emerald-900/60 dark:bg-emerald-950/30">
        <span className="font-medium">Golden Rig v{golden.version}</span>
        <span className="text-neutral-600 dark:text-neutral-400">
          {golden.from_this_machine
            ? " was made from this machine."
            : " comes from another machine. Comparing this one against it is next."}
        </span>
      </section>
    );
  }
  return (
    <section className="flex items-center justify-between gap-4 rounded-lg border border-neutral-200 px-4 py-3 dark:border-neutral-800">
      <div className="text-sm">
        <div className="font-medium">Make this your Golden Rig?</div>
        <div className="text-neutral-500">
          Your other machines will be compared against this setup. You can change it later.
        </div>
      </div>
      <Button onClick={onMakeGolden} disabled={busy}>
        Make Golden
      </Button>
    </section>
  );
}
