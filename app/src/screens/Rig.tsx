import { BackupLine } from "../Backup";
import { BigFiles } from "../BigFiles";
import { useEffect, useMemo, useState } from "react";
import {
  FORMAT_LABEL,
  openVendorApp,
  sourceLabel,
  updates,
  vendorApps,
  type Overview,
  type PluginFormat,
  type ProductRow,
  type UpdateAlert,
  type VendorApp,
} from "../api";
import { Badge, Button, Folder, FolderControls, inputClass, Pill, timeAgo, useFolders } from "../ui";

type Grouping = "vendor" | "format";

interface Props {
  overview: Overview;
  warnings: string[];
  busy: boolean;
  onRescan: () => void;
  onMakeGolden: () => void;
  onReport: () => void;
  onError: (e: string) => void;
}

export function Rig({ overview, warnings, busy, onRescan, onMakeGolden, onReport, onError }: Props) {
  const { machine, inventory: inv, golden } = overview;
  const [grouping, setGrouping] = useState<Grouping>("vendor");
  const [query, setQuery] = useState("");
  const [alerts, setAlerts] = useState<UpdateAlert[]>([]);

  // Reloads with the overview, which refreshes after scans, syncs and catalog updates.
  useEffect(() => {
    updates()
      .then((u) => setAlerts(u.alerts))
      .catch(() => setAlerts([]));
  }, [overview]);
  const alertById = useMemo(() => new Map(alerts.map((a) => [a.id, a])), [alerts]);
  const [launchable, setLaunchable] = useState<VendorApp[]>([]);
  useEffect(() => {
    vendorApps().then((a) => setLaunchable(a.filter((x) => x.can_open)), () => {});
  }, [overview]);

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
  const folders = useFolders(`rig.${grouping}`);
  const filtering = query.trim() !== "";

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
            {plural(inv.templates, "Reaper template")}
          </p>
        </div>
        <Button variant="quiet" onClick={onReport} title="A summary of this machine to share">
          Share Rig Report
        </Button>
        <Button variant="quiet" onClick={onRescan} disabled={busy}>
          Rescan
        </Button>
      </header>

      <GoldenCard golden={golden} busy={busy} onMakeGolden={onMakeGolden} />
      <BackupLine onError={onError} fresh={!golden} />

      {/* Errors here surface where the files are listed, on Up To Speed. */}
      <BigFiles onError={() => {}} className="mx-4 mt-2" />

      {warnings.length > 0 && (
        <details className="border-b border-line px-4 py-2 text-xs">
          <summary className="cursor-pointer text-warn">
            {warnings.length} plug-in file{warnings.length === 1 ? "" : "s"} had no readable version info
          </summary>
          <p className="mt-1 text-dim">Still listed, identified by file name.</p>
          <ul className="mt-1.5 max-h-40 overflow-auto font-mono text-[11px] text-dim">
            {warnings.map((w) => (
              <li key={w}>{w}</li>
            ))}
          </ul>
        </details>
      )}

      {alerts.length > 0 && <Updates alerts={alerts} />}

      <div className="flex items-end gap-3 pl-4 pr-3 pt-3">
        <div className="min-w-0 flex-1">
          <h2 className="text-sm font-semibold">
            Plug-ins installed on this machine{" "}
            <span className="font-normal tabular-nums text-dim">{inv.products.toLocaleString()}</span>
          </h2>
          <p className="text-xs text-dim">
            {plural(vendorCount, "vendor")} · {plural(inv.plugins, "file")} in the standard plug-in folders. Read
            only: nothing is loaded or changed.
          </p>
        </div>
        {!filtering && <FolderControls ids={groups.map((g) => g.title)} folders={folders} className="" />}
      </div>

      <div className="flex flex-wrap items-center gap-1 px-3 pb-1 pt-2">
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
          placeholder="Filter by name or vendor"
          aria-label="Filter plug-ins"
          className={`ml-auto w-48 ${inputClass}`}
        />
      </div>

      {groups.length === 0 && (
        <p className="px-4 py-3 text-dim">
          {inv.products === 0 ? "No plug-ins found in the standard folders." : "Nothing matches."}
        </p>
      )}
      {groups.map((g) => {
        const updateCount = g.rows.filter(({ p }) => alertById.has(p.id)).length;
        return (
          <Folder
            key={g.title}
            title={g.title}
            count={g.rows.length}
            open={filtering || folders.isOpen(g.title)}
            onToggle={() => folders.toggle(g.title)}
            badges={
              updateCount > 0 && (
                <Badge>
                  {updateCount} update{updateCount === 1 ? "" : "s"}
                </Badge>
              )
            }
          >
            <ul className="pb-1">
              {g.rows.map(({ vendor, p }) => (
                <ProductLine
                  key={`${g.title}/${p.id}`}
                  p={p}
                  vendor={grouping === "format" ? vendor : null}
                  update={alertById.get(p.id)}
                />
              ))}
            </ul>
          </Folder>
        );
      })}

      {inv.presets.length > 0 && (
        <section>
          <h3 className="px-4 pb-0.5 pt-4 text-[11px] text-dim">
            Preset folders · compared with your other machines; linked machines can bring them over
          </h3>
          <ul>
            {inv.presets.map((p) => (
              <li key={p.name} className="flex h-7 items-center justify-between px-4 hover:bg-sel/60">
                <span>{p.name}</span>
                <span className="font-mono text-[11px] text-dim">{formatSize(p.size)}</span>
              </li>
            ))}
          </ul>
        </section>
      )}

      {inv.apps.length > 0 && (
        <section>
          <h3 className="px-4 pb-0.5 pt-4 text-[11px] text-dim">Vendor apps and licensing tools installed</h3>
          <ul>
            {inv.apps.map((a) => (
              <li key={a.name} className="group flex h-7 items-center gap-3 px-4 hover:bg-sel/60">
                <span className="flex-1">{a.name}</span>
                {launchable.some((x) => x.name === a.name) && (
                  <Button
                    variant="quiet"
                    className="h-6 px-2 opacity-0 group-hover:opacity-100 focus:opacity-100"
                    onClick={() => openVendorApp(launchable.find((x) => x.name === a.name)!.id).catch(() => {})}
                  >
                    Open
                  </Button>
                )}
                <span className="font-mono text-[11px] text-dim">{a.version ?? ""}</span>
              </li>
            ))}
          </ul>
        </section>
      )}
    </div>
  );
}

function formatSize(bytes: number | null): string {
  if (bytes === null) return "";
  if (bytes < 1024 * 1024) return `${Math.max(1, Math.round(bytes / 1024))} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
  return `${(bytes / 1024 / 1024 / 1024).toFixed(1)} GB`;
}

function Updates({ alerts }: { alerts: UpdateAlert[] }) {
  return (
    <details className="border-b border-line px-4 py-2 text-xs">
      <summary className="cursor-pointer">
        <span className="font-medium text-accent-text">
          {alerts.length} plug-in update{alerts.length === 1 ? "" : "s"} available
        </span>
        <span className="text-dim"> · a newer version is on another machine, the Primary Rig or the catalog</span>
      </summary>
      <ul className="mt-1.5">
        {alerts.map((a) => (
          <li key={a.id} className="flex h-7 items-center gap-3">
            <span className="min-w-0 flex-1 truncate">
              {a.name}
              {a.vendor && <span className="ml-2 text-dim">{a.vendor}</span>}
            </span>
            <span className="font-mono text-[11px] text-dim">
              {a.installed} → <span className="text-fg">{a.latest}</span>
            </span>
            <span className="w-36 truncate text-right text-[11px] text-dim">{sourceLabel(a.source)}</span>
          </li>
        ))}
      </ul>
    </details>
  );
}

function ProductLine({ p, vendor, update }: { p: ProductRow; vendor: string | null; update?: UpdateAlert }) {
  return (
    <li className="flex h-7 items-center gap-3 pl-8 pr-4 hover:bg-sel/60">
      <span className="min-w-0 flex-1 truncate">
        {p.name}
        {vendor && <span className="ml-2 text-xs text-dim">{vendor}</span>}
      </span>
      {p.confidence === "low" && (
        <span title="Identified from the file name only">
          <Badge tone="amber">name only</Badge>
        </span>
      )}
      {update && (
        <span title={`${update.latest} ${sourceLabel(update.source)}`}>
          <Badge>update</Badge>
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
          <span className="font-medium text-fg">Primary Rig v{golden.version}</span>
          {golden.from_this_machine
            ? " was made from this machine. Your other machines are compared to it."
            : " comes from another machine. Compare shows what this one is missing."}
        </span>
      </p>
    );
  }
  return (
    <section className="flex items-center gap-3 border-b border-line px-4 py-2 text-xs">
      <span className="min-w-0 flex-1 text-dim">
        <span className="font-medium text-fg">Make this your Primary Rig?</span> Your other machines will be
        compared against this setup. You can change it later.
      </span>
      <Button onClick={onMakeGolden} disabled={busy}>
        Make Primary
      </Button>
    </section>
  );
}
