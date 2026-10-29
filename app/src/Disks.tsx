import type { Disk, Os } from "./api";
import { Notice, timeAgo } from "./ui";

// Drive space on every machine (decision #101): the drives holding its
// plug-ins, presets, DAW files, home folder and Installer Vault.

const HOLDS: Record<string, string> = {
  plugins: "plug-ins",
  presets: "presets",
  daw: "DAW files",
  home: "home folder",
  vault: "Installer Vault",
};

/**
 * "412 GB", "1.2 TB", in the units that machine's own drive bars use:
 * Explorer counts in 1024s, Finder in 1000s. Rounded down, so free space is
 * never overstated.
 */
export function formatBytes(bytes: number, os: Os): string {
  const k = os === "windows" ? 1024 : 1000;
  for (const [unit, size] of [
    ["TB", k ** 4],
    ["GB", k ** 3],
    ["MB", k ** 2],
  ] as const) {
    if (bytes >= size) {
      const n = bytes / size;
      return n < 10 ? `${(Math.floor(n * 10) / 10).toFixed(1)} ${unit}` : `${Math.floor(n)} ${unit}`;
    }
  }
  return `${Math.ceil(bytes / k)} KB`;
}

/** Nearly full: under 10 GB, or under a tenth and under 50 GB. */
export function isLow(d: Disk): boolean {
  return d.free < 10e9 || (d.free < d.total / 10 && d.free < 50e9);
}

/** Drives holding something Patch Pilot uses; a full backup or archive drive isn't worth a warning. */
function inUse(disks: Disk[]): Disk[] {
  const used = disks.filter((d) => d.holds.length > 0);
  return used.length > 0 ? used : disks;
}

/** The drive to name when there's room for one: the fullest in use if one is nearly full, else the home folder's. */
export function headlineDisk(disks: Disk[]): Disk | null {
  return inUse(disks).find(isLow) ?? disks[0] ?? null;
}

/** "Samples (D:)" like Explorer when the drive has a label, else "D:" or the Mac volume's name. */
export function diskLabel(d: Disk): string {
  return d.label ? `${d.label} (${d.name})` : d.name;
}

/** What a drive holds, in words: "plug-ins, presets and DAW files". */
function holdsLabel(d: Disk): string {
  const parts = d.holds.map((h) => HOLDS[h]).filter(Boolean);
  return parts.length > 1 ? `${parts.slice(0, -1).join(", ")} and ${parts[parts.length - 1]}` : (parts[0] ?? "");
}

/** Every drive on one line each, for a tooltip. */
export function disksTitle(disks: Disk[], os: Os): string {
  return disks
    .map((d) => `${diskLabel(d)}: ${formatBytes(d.free, os)} free of ${formatBytes(d.total, os)}${d.holds.length ? ` (${holdsLabel(d)})` : ""}`)
    .join("\n");
}

/**
 * "412 GB free", or "D: 8 GB free" in amber when a drive is nearly full. Names
 * the drive when the machine has more than one. Nothing before a machine has
 * been measured (an older Patch Pilot there).
 */
export function DiskSummary({
  disks,
  os,
  named = true,
  className = "",
}: {
  disks: Disk[];
  os: Os;
  /** False leaves out the drive's name where space is tight; the tooltip has every drive. */
  named?: boolean;
  className?: string;
}) {
  const d = headlineDisk(disks);
  if (!d) return null;
  return (
    <span className={`${isLow(d) ? "text-warn" : ""} ${className}`} title={disksTitle(disks, os)}>
      {named && disks.length > 1 ? `${d.name} ` : ""}
      {formatBytes(d.free, os)} free
    </span>
  );
}

/** A thin track filled to how full the drive is; amber when nearly full. */
export function DiskBar({ disk, className = "" }: { disk: Disk; className?: string }) {
  const used = disk.total > 0 ? Math.min(100, Math.max(0, (1 - disk.free / disk.total) * 100)) : 0;
  return (
    <div className={`h-[3px] overflow-hidden rounded-full bg-track ${className}`} aria-hidden="true">
      <div className={`h-full rounded-full ${isLow(disk) ? "bg-warn" : "bg-accent"}`} style={{ width: `${used}%` }} />
    </div>
  );
}

/**
 * Each drive with its bar, free of total and what it holds. `showWhen` adds
 * when it was measured, for another machine, whose numbers are from its last check.
 */
export function DiskList({
  disks,
  os,
  className = "",
  showWhen = false,
}: {
  disks: Disk[];
  os: Os;
  className?: string;
  showWhen?: boolean;
}) {
  const when = Math.max(...disks.map((d) => d.measured_at));
  return (
    <div className={`flex flex-col gap-2.5 ${className}`}>
      {disks.map((d) => (
        <div key={d.name} className="flex flex-col gap-1">
          <div className="flex items-baseline gap-2 text-xs">
            <span className="min-w-0 flex-1 truncate font-medium" title={diskLabel(d)}>
              {diskLabel(d)}
            </span>
            <span className={`shrink-0 tabular-nums ${isLow(d) ? "text-warn" : "text-dim"}`}>
              {formatBytes(d.free, os)} free of {formatBytes(d.total, os)}
            </span>
          </div>
          <DiskBar disk={d} />
          {d.holds.length > 0 && <span className="text-[11px] leading-snug text-dim">{holdsLabel(d)}</span>}
        </div>
      ))}
      {showWhen && disks.length > 0 && <span className="text-[11px] text-dim">Checked {timeAgo(when)}</span>}
    </div>
  );
}

/** A warn line on this machine's screens when a drive is nearly full: installs and bring-overs need room. */
export function LowDiskNotice({ disks, os }: { disks: Disk[]; os: Os }) {
  const low = inUse(disks).filter(isLow);
  if (low.length === 0) return null;
  const names = low.map((d) => `${diskLabel(d)} has ${formatBytes(d.free, os)} free`).join(", ");
  return (
    <Notice
      tone="warn"
      text={
        <>
          <span className="font-medium">Running out of space.</span>
          <span className="text-dim"> {names}. Installers and big files may not fit.</span>
        </>
      }
    />
  );
}
