import { BackupLine } from "../Backup";
import { BigFiles } from "../BigFiles";
import { useEffect, useMemo, useState, type ReactNode } from "react";
import {
  FORMAT_LABEL,
  openVendorApp,
  sourceLabel,
  updates,
  vendorApps,
  type Overview,
  type PluginFormat,
  type ProductRow,
  type Disk,
  type Os,
  type UpdateAlert,
  type VendorApp,
} from "../api";
import { DiskBar, DiskList, DiskSummary, headlineDisk, LowDiskNotice } from "../Disks";
import { ReaperExtensionsNotice } from "../ReaperExtensions";
import { ReaperStartupNotice } from "../ReaperStartup";
import { Duplicates } from "../Duplicates";
import {
  Badge,
  Button,
  Folder,
  FolderControls,
  Glyph,
  inputClass,
  Label,
  Notice,
  Pill,
  Tile,
  timeAgo,
  useFolders,
} from "../ui";

type Grouping = "vendor" | "format";

interface Props {
  overview: Overview;
  /** The "Set up this machine" line, shown with the other notices. */
  setupOffer?: ReactNode;
  warnings: string[];
  busy: boolean;
  onRescan: () => void;
  onMakeGolden: () => void;
  onReport: () => void;
  onError: (e: string) => void;
  /** When the tray asked to set up syncing; opens the Sync & backup line. */
  syncSetupAt?: number;
}

export function Rig({
  overview,
  setupOffer,
  warnings,
  busy,
  onRescan,
  onMakeGolden,
  onReport,
  onError,
  syncSetupAt,
}: Props) {
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
  const extra = useFolders("rig.extra");
  // Short, and what people look for, so open until closed.
  const drives = useFolders("rig.drives", () => true);
  const [warningsOpen, setWarningsOpen] = useState(false);
  const filtering = query.trim() !== "";

  if (!machine || !inv) return null;

  const presetBytes = inv.presets.reduce((n, p) => n + (p.size ?? 0), 0);
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

      {/* One line each, in one place: what this machine is to the Primary Rig, then anything to do. */}
      <div className="flex flex-col border-b border-line px-2 py-1.5">
        <GoldenLine golden={golden} busy={busy} onMakeGolden={onMakeGolden} />
        <BackupLine onError={onError} fresh={!golden} openAt={syncSetupAt} />
        {setupOffer}
        {/* Errors here surface where the files are listed, on Up To Speed. */}
        <BigFiles onError={() => {}} compact />
        <LowDiskNotice disks={machine.disks} os={machine.os} />
        <ReaperExtensionsNotice onError={onError} />
        <ReaperStartupNotice onError={onError} />
        {alerts.length > 0 && <Updates alerts={alerts} />}
        {/* Only real read problems land here; a plug-in with no version number is normal. */}
        {warnings.length > 0 && (
          <Notice
            tone="dim"
            text={`${plural(warnings.length, "file")} couldn't be read fully. Still listed, by name.`}
            open={warningsOpen}
            onToggle={() => setWarningsOpen((o) => !o)}
          >
            <ul className="max-h-32 overflow-auto pb-1 pl-6 text-[11px] text-dim">
              {warnings.map((w) => {
                const { name, reason } = splitWarning(w);
                return (
                  <li key={w} className="truncate" title={w}>
                    {name}
                    {reason && <span className="opacity-70"> · {reason}</span>}
                  </li>
                );
              })}
            </ul>
          </Notice>
        )}
      </div>

      {/* The whole plug-in list folds into one folder (closed at first) so the screen stays short. */}
      <div className="pt-2">
        <Folder
          title="All plug-ins"
          count={inv.products}
          open={extra.isOpen("plugins")}
          onToggle={() => extra.toggle("plugins")}
          badges={
            <>
              {alerts.length > 0 && (
                <Badge>
                  {alerts.length} update{alerts.length === 1 ? "" : "s"}
                </Badge>
              )}
              <span className="text-[11px] text-dim">
                {plural(vendorCount, "vendor")} · {plural(inv.plugins, "file")} · read only
              </span>
            </>
          }
        >
          <div className="flex flex-wrap items-center gap-1 pb-1 pl-8 pr-3 pt-1">
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
            <span className="flex-1" />
            {!filtering && <FolderControls ids={groups.map((g) => g.title)} folders={folders} className="" />}
            <input
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder="Filter by name or vendor"
              aria-label="Filter plug-ins"
              className={`w-48 ${inputClass}`}
            />
          </div>

          {groups.length === 0 && (
            <p className="py-3 pl-8 pr-4 text-dim">
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
                className="pr-4"
                indent={32}
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
        </Folder>
      </div>

      <Label className="px-4 pb-1 pt-4">Also on this machine</Label>
      <div className="mx-3 flex flex-col divide-y divide-line overflow-hidden rounded-lg border border-line bg-panel">
        {machine.disks.length > 0 && (
          <Folder
            title="Drives"
            count={machine.disks.length}
            icon={<Tile name="drive" />}
            className="pl-3 pr-4"
            open={drives.isOpen("drives")}
            onToggle={() => drives.toggle("drives")}
            badges={<DriveBadge disks={machine.disks} os={machine.os} />}
          >
            <DiskList disks={machine.disks} os={machine.os} grid className="pb-3 pl-16 pr-4 pt-1" />
          </Folder>
        )}
        {inv.presets.length > 0 && (
          <Folder
            title="Preset folders"
            count={inv.presets.length}
            icon={<Tile name="folder" />}
            className="pl-3 pr-4"
            open={extra.isOpen("presets")}
            onToggle={() => extra.toggle("presets")}
            badges={
              <span className="text-[11px] text-dim">
                {presetBytes > 0 && <span className="tabular-nums">{formatSize(presetBytes)} · </span>}
                Linked machines can bring them over
              </span>
            }
          >
            <ul className="pb-1.5">
              {inv.presets.map((p) => (
                <li key={p.name} className="flex h-7 items-center gap-2 pl-16 pr-4 hover:bg-sel/60">
                  <Glyph name="folder" size={12} className="text-dim" />
                  <span className="min-w-0 flex-1 truncate">{p.name}</span>
                  <span className="font-mono text-[11px] text-dim">{formatSize(p.size)}</span>
                </li>
              ))}
            </ul>
          </Folder>
        )}
        <Duplicates />
        {inv.apps.length > 0 && (
          <Folder
            title="Vendor apps and licensing tools"
            count={inv.apps.length}
            icon={<Tile name="apps" />}
            className="pl-3 pr-4"
            open={extra.isOpen("apps")}
            onToggle={() => extra.toggle("apps")}
            badges={<span className="text-[11px] text-dim">{appsSummary(inv.apps)}</span>}
          >
            <ul className="pb-1.5">
              {inv.apps.map((a) => (
                <li key={a.name} className="group flex h-7 items-center gap-2 pl-16 pr-4 hover:bg-sel/60">
                  <Glyph name={a.kind === "license_tool" ? "key" : "apps"} size={12} className="text-dim" />
                  <span className="min-w-0 flex-1 truncate">{a.name}</span>
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
          </Folder>
        )}
      </div>
    </div>
  );
}

/** The Drives row's end: the headline drive's free space after a short bar of how full it is. */
function DriveBadge({ disks, os }: { disks: Disk[]; os: Os }) {
  const d = headlineDisk(disks);
  if (!d) return null;
  return (
    <span className="flex items-center gap-2.5 text-[11px] text-dim">
      <DiskBar disk={d} className="w-16" />
      <DiskSummary disks={disks} os={os} />
    </span>
  );
}

/** "1 licensing tool · 12 vendor apps". */
function appsSummary(apps: { kind: string }[]): string {
  const tools = apps.filter((a) => a.kind === "license_tool").length;
  const others = apps.length - tools;
  const parts = [];
  if (tools > 0) parts.push(`${tools} licensing tool${tools === 1 ? "" : "s"}`);
  if (others > 0) parts.push(`${others} vendor app${others === 1 ? "" : "s"}`);
  return parts.join(" · ");
}

function formatSize(bytes: number | null): string {
  if (bytes === null) return "";
  if (bytes < 1024 * 1024) return `${Math.max(1, Math.round(bytes / 1024))} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
  return `${(bytes / 1024 / 1024 / 1024).toFixed(1)} GB`;
}

function Updates({ alerts }: { alerts: UpdateAlert[] }) {
  const [open, setOpen] = useState(false);
  return (
    <Notice
      text={`${alerts.length} plug-in update${alerts.length === 1 ? "" : "s"} available`}
      open={open}
      onToggle={() => setOpen((o) => !o)}
    >
      <ul>
        {alerts.map((a) => (
          <li key={a.id} className="flex h-6 items-center gap-3">
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
    </Notice>
  );
}

function ProductLine({ p, vendor, update }: { p: ProductRow; vendor: string | null; update?: UpdateAlert }) {
  return (
    <li className="flex h-7 items-center gap-3 pl-14 pr-4 hover:bg-sel/60">
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

function GoldenLine({
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
      <Notice
        tone="ok"
        text={
          <>
            <span className="font-medium">Primary Rig v{golden.version}</span>
            <span className="text-dim">
              {golden.from_this_machine
                ? " was made from this machine. Your other machines are compared to it."
                : " comes from another machine. Compare shows what this one is missing."}
            </span>
          </>
        }
      />
    );
  }
  return (
    <Notice
      text={
        <>
          <span className="font-medium">Make this your Primary Rig?</span>
          <span className="text-dim"> Your other machines will be compared against it. You can change it later.</span>
        </>
      }
      actions={
        <Button onClick={onMakeGolden} disabled={busy} className="h-6 shrink-0 px-2.5">
          Make Primary
        </Button>
      }
    />
  );
}

/** "C:\…\Acme.vst3: invalid moduleinfo.json: …" → the file name and the reason. */
function splitWarning(w: string): { name: string; reason: string } {
  const at = w.indexOf(": ");
  if (at < 0) return { name: w, reason: "" };
  const path = w.slice(0, at);
  return { name: path.split(/[\\/]/).pop() || path, reason: w.slice(at + 2) };
}
