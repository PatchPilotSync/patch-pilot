import { useEffect, useRef, useState, type ReactNode } from "react";
import { backupView, setBackupFolder, setKeepInSync, type BackupView } from "./api";
import { FolderPicker } from "./BigFiles";
import { Badge, Button, Glyph, Mark, ScoreBar, scoreColor, Tile, type GlyphName } from "./ui";

// The first-run tutorial (decision #36): a short, skippable card that explains
// the core ideas. Shown once on first launch, reopened from "How it works".
// The illustrations are tiny mock-ups built from the app's own pieces, so they
// follow the theme without any images.

const SEEN_KEY = "patchpilot.tour.v1";

/** True until the tutorial has been finished or skipped once. */
export function tourPending(): boolean {
  try {
    return localStorage.getItem(SEEN_KEY) === null;
  } catch {
    return false;
  }
}

/** Forget that the tutorial was seen, so it shows as on a first launch. */
export function resetTour() {
  try {
    localStorage.removeItem(SEEN_KEY);
  } catch {
    // Nothing stored, nothing to reset.
  }
}

function markSeen() {
  try {
    localStorage.setItem(SEEN_KEY, "seen");
  } catch {
    // Private or blocked storage: the tutorial just shows again next launch.
  }
}

/** Where the icon lives: "menu bar" on macOS, "system tray" on Windows. */
const TRAY = /Mac/i.test(navigator.userAgent) ? "menu bar" : "system tray";

interface Step {
  title: string;
  body: ReactNode;
  art: ReactNode;
}

const STEPS: Step[] = [
  {
    title: "Every rig, ready for the session",
    body: "Patch Pilot keeps your studio, laptop and travel machines matching, so a project opens the same on each. It only reads what's installed, and never copies plug-ins or touches a license.",
    art: <MachinesArt />,
  },
  {
    title: "The Primary Rig",
    body: "Your ideal setup, made from your best machine. Every other machine is measured against it, and each version is kept.",
    art: <GoldenArt />,
  },
  {
    title: "Compare and the parity score",
    body: "Compare shows what each machine lacks next to the Primary Rig. Under the score, “Why 87, not 100” lists each reason with the points it costs.",
    art: <CompareArt />,
  },
  {
    title: "Get up to speed",
    body: "A checklist of what's Not installed or needs an Update, with both versions. Open the vendor app or a Vault installer from it; Patch Pilot never installs anything itself. Ignore what this machine doesn't need.",
    art: <ChecklistArt />,
  },
  {
    title: "Keep your files in sync",
    body: <SyncStep />,
    art: <SyncArt />,
  },
  {
    title: "Nothing is lost",
    body: "Every change is a restore point in History, so you can go back. This machine finds duplicate presets to clean up, and the Vault keeps installers ready for your next machine.",
    art: <HistoryArt />,
  },
  {
    title: `Lives in your ${TRAY}`,
    body: `Click the plane in the ${TRAY} for every machine's score, or Expand for this window. It starts with your computer and rescans on its own when plug-in folders change.`,
    art: <TrayArt />,
  },
];

export function Tour({ onClose }: { onClose: () => void }) {
  const [i, setI] = useState(0);
  const card = useRef<HTMLDivElement>(null);
  const step = STEPS[i];
  const last = i === STEPS.length - 1;

  const closeRef = useRef(onClose);
  closeRef.current = onClose;
  function close() {
    markSeen();
    closeRef.current();
  }

  useEffect(() => {
    card.current?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        markSeen();
        closeRef.current();
      }
      else if (e.key === "ArrowRight") setI((n) => Math.min(n + 1, STEPS.length - 1));
      else if (e.key === "ArrowLeft") setI((n) => Math.max(n - 1, 0));
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/30 p-4" onClick={close}>
      <div
        ref={card}
        role="dialog"
        aria-modal="true"
        aria-labelledby="tour-title"
        tabIndex={-1}
        onClick={(e) => e.stopPropagation()}
        className="flex w-[380px] max-w-full flex-col overflow-hidden rounded-lg border border-line bg-panel shadow-xl outline-none"
      >
        <div className="flex h-[150px] items-center justify-center border-b border-line bg-canvas">{step.art}</div>
        <div className="flex min-h-[160px] flex-col px-5 pt-4">
          <p className="text-[11px] text-dim">
            {i + 1} of {STEPS.length}
          </p>
          <h2 id="tour-title" className="mt-0.5 text-[15px] font-semibold">
            {step.title}
          </h2>
          <div className="mt-1.5 leading-relaxed text-dim">{step.body}</div>
        </div>
        <footer className="flex h-12 items-center gap-1 px-3">
          <Button variant="quiet" onClick={close}>
            {last ? "Close" : "Skip"}
          </Button>
          <span className="flex flex-1 justify-center gap-1.5" aria-hidden="true">
            {STEPS.map((_, n) => (
              <span key={n} className={`h-1.5 w-1.5 rounded-full ${n === i ? "bg-accent" : "bg-track"}`} />
            ))}
          </span>
          {i > 0 && (
            <Button variant="quiet" onClick={() => setI(i - 1)}>
              Back
            </Button>
          )}
          <Button onClick={() => (last ? close() : setI(i + 1))}>{last ? "Get started" : "Next"}</Button>
        </footer>
      </div>
    </div>
  );
}

/**
 * Onboarding asks which cloud service the user has (decision #70): picking
 * its folder turns on syncing, so new templates, scripts and presets reach
 * every machine through it with no clicks.
 */
function SyncStep() {
  const [view, setView] = useState<BackupView | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    backupView().then(setView, () => {});
  }, []);

  async function choose(service: string | null, folder: string | null) {
    setBusy(true);
    setError(null);
    try {
      await setBackupFolder(service, folder);
      await setKeepInSync(true);
      setView(await backupView());
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  const intro =
    "Templates, scripts, presets and DAW settings reach your other machines on their own. Choose what syncs on each machine; Reaper shortcuts move between PC and Mac keys.";
  if (view?.github && view.settings.keep_in_sync) {
    return (
      <>
        {intro}
        <span className="mt-2 block text-ok">✓ Syncing through GitHub ({view.github}).</span>
      </>
    );
  }
  if (view?.dir && view.settings.keep_in_sync) {
    const cloud = view.clouds.find((c) => c.path === view.settings.folder)?.name ?? "your folder";
    return (
      <>
        {intro}
        <span className="mt-2 block text-ok">
          ✓ Syncing through {cloud}. Change it under Sync &amp; backup on This machine.
        </span>
      </>
    );
  }
  return (
    <>
      {intro}
      {view && (
        <span className="mt-2 block">
          <FolderPicker
            clouds={view.clouds}
            busy={busy}
            missing={false}
            preferred={view.settings.folder ?? view.transfer_folder}
            onChoose={choose}
            hint="Which cloud service do you use? Pick the same one on each machine, or GitHub later on This machine."
          />
        </span>
      )}
      {error && <span className="mt-1 block text-warn">{error}</span>}
    </>
  );
}

// Illustrations. Small, token-coloured, decorative only.

function Chip({ children, className = "" }: { children: ReactNode; className?: string }) {
  return (
    <div className={`flex h-7 items-center gap-2 rounded-md border border-line bg-panel px-2.5 text-xs ${className}`}>
      {children}
    </div>
  );
}

/** A small line icon in a tone's colour, as the app's rows show them. */
function Icon({ name, tone }: { name: GlyphName; tone: "ok" | "warn" | "bad" | "dim" | "accent" }) {
  const colour = { ok: "text-ok", warn: "text-warn", bad: "text-bad", dim: "text-dim", accent: "text-accent-text" }[tone];
  return <Glyph name={name} size={12} className={colour} />;
}

function MachinesArt() {
  return (
    <div className="flex items-center gap-4" aria-hidden="true">
      <div className="flex flex-col gap-1.5">
        <Chip>
          <Icon name="desktop" tone="ok" /> Studio PC
        </Chip>
        <Chip>
          <Icon name="desktop" tone="ok" /> MacBook
        </Chip>
        <Chip>
          <Icon name="desktop" tone="warn" /> Travel laptop
        </Chip>
      </div>
      <svg width="28" height="60" viewBox="0 0 28 60" fill="none" className="text-line">
        <path d="M0 8 H10 Q14 8 14 14 V46 Q14 52 10 52 H0 M0 30 H28" stroke="currentColor" strokeWidth="1.5" />
      </svg>
      <div className="flex h-14 w-14 items-center justify-center rounded-lg border border-line bg-panel">
        <Mark size={32} className="text-accent-text" />
      </div>
    </div>
  );
}

function GoldenArt() {
  return (
    <div className="w-[220px] rounded-md border border-line bg-panel py-1 text-xs" aria-hidden="true">
      <div className="flex items-center gap-2 border-b border-line px-1.5 pb-1">
        <Tile name="star" tone="ok" className="" />
        <span className="font-semibold">Primary Rig</span>
        <span className="flex-1" />
        <Badge>v3</Badge>
      </div>
      {[
        ["Pro-Q 4", "4.0.2"],
        ["Serum 2", "2.0.14"],
        ["Kontakt 8", "8.4.1"],
      ].map(([name, v]) => (
        <div key={name} className="flex h-6 items-center gap-2 px-2.5">
          <Icon name="check" tone="ok" />
          <span className="flex-1">{name}</span>
          <span className="text-[11px] text-dim">{v}</span>
        </div>
      ))}
    </div>
  );
}

function CompareArt() {
  return (
    <div className="flex items-center gap-4" aria-hidden="true">
      <div className="w-[64px] text-center">
        <div className="text-[28px] font-semibold leading-none text-warn tabular-nums">87</div>
        <ScoreBar score={87} className="mt-2" />
        <div className="mt-1.5 text-[11px] text-dim">parity</div>
      </div>
      <div className="w-[190px] rounded-md border border-line bg-panel py-1 text-xs">
        <div className="px-2.5 py-0.5 text-[11px] text-dim">Why 87, not 100</div>
        {(
          [
            ["x", "bad", "Serum 2 is missing", "−8"],
            ["download", "warn", "Pro-Q 4 is older", "−3"],
            ["pencil", "warn", "2 presets differ", "−2"],
          ] as const
        ).map(([glyph, tone, label, pts]) => (
          <div key={label} className="flex h-6 items-center gap-2 px-2.5">
            <Icon name={glyph} tone={tone} />
            <span className="flex-1">{label}</span>
            <span className="text-[11px] text-dim tabular-nums">{pts}</span>
          </div>
        ))}
      </div>
    </div>
  );
}

function ChecklistArt() {
  return (
    <div className="w-[250px] rounded-md border border-line bg-panel py-1 text-xs" aria-hidden="true">
      <div className="px-2.5 py-1 text-[11px] text-dim">Travel laptop needs</div>
      {(
        [
          [true, "iLok License Manager", null],
          [false, "Serum 2", "Not installed"],
          [false, "Pro-Q 4", "Update 4.0.1 → 4.0.2"],
        ] as const
      ).map(([done, label, tag]) => (
        <div key={label} className="flex h-6 items-center gap-2 px-2.5">
          <span
            className={`flex h-3.5 w-3.5 shrink-0 items-center justify-center rounded-[4px] ${
              done ? "bg-ok/15 text-ok" : "border border-dim/50"
            }`}
          >
            {done && <Glyph name="check" size={10} />}
          </span>
          <span className={`flex-1 ${done ? "text-dim line-through" : ""}`}>{label}</span>
          {tag && <span className="text-[11px] text-dim">{tag}</span>}
        </div>
      ))}
      <div className="flex h-6 items-center gap-2 px-2.5 text-dim">
        <span className="h-3.5 w-3.5 shrink-0" />
        <span className="flex-1 line-through">Kontakt 8 AAX</span>
        <span className="text-[11px]">Ignored</span>
      </div>
    </div>
  );
}

function SyncArt() {
  return (
    <div className="flex flex-col items-center gap-2.5" aria-hidden="true">
      <div className="flex items-center gap-3">
        <Chip>
          <Icon name="desktop" tone="ok" /> MacBook
        </Chip>
        <span className="text-dim">→</span>
        <div className="flex h-12 w-12 items-center justify-center rounded-lg border border-line bg-panel">
          <svg width="26" height="18" viewBox="0 0 26 18" fill="none" className="text-accent-text">
            <path
              d="M7 17 H20 A5 5 0 0 0 20.5 7 A7 7 0 0 0 7.2 5.5 A5.8 5.8 0 0 0 7 17 Z"
              stroke="currentColor"
              strokeWidth="1.5"
              strokeLinejoin="round"
            />
          </svg>
        </div>
        <span className="text-dim">→</span>
        <Chip>
          <Icon name="desktop" tone="ok" /> Studio PC
        </Chip>
      </div>
      <div className="flex gap-1.5 text-[11px] text-dim">
        <Badge>Templates</Badge>
        <Badge>Presets</Badge>
        <Badge>Shortcuts ⌘H ↔ Ctrl+H</Badge>
      </div>
    </div>
  );
}

function HistoryArt() {
  return (
    <div className="w-[250px] rounded-md border border-line bg-panel py-1 text-xs" aria-hidden="true">
      {(
        [
          ["copies", "Synced 3 presets from PC", "2 min"],
          ["copies", "Cleaned up 12 duplicates", "1 h"],
          ["search", "Serum 2 installed", "Mon"],
        ] as const
      ).map(([glyph, label, when], n) => (
        <div key={label} className="flex h-6 items-center gap-2 px-2.5">
          <Icon name={glyph} tone={n === 2 ? "dim" : "accent"} />
          <span className="flex-1 truncate">{label}</span>
          <span className={`text-[11px] ${n === 1 ? "text-accent-text" : "text-dim"}`}>{n === 1 ? "Go back…" : when}</span>
        </div>
      ))}
    </div>
  );
}

function TrayArt() {
  const rows = [
    ["Studio PC", 100],
    ["Travel laptop", 87],
  ] as const;
  return (
    <div className="w-[230px] overflow-hidden rounded-md border border-line bg-canvas text-xs shadow-sm" aria-hidden="true">
      <div className="flex h-7 items-center gap-1.5 border-b border-line bg-panel px-2.5">
        <Mark size={12} className="text-accent-text" />
        <span className="font-semibold">Patch Pilot</span>
        <span className="flex-1" />
        <span className="text-[10px] text-dim">Scanned just now</span>
      </div>
      <div className="m-2 flex flex-col divide-y divide-line rounded-md border border-line bg-panel">
        {rows.map(([name, score]) => (
          <div key={name} className="flex h-7 items-center gap-2 px-2">
            <Icon name="desktop" tone={score >= 95 ? "ok" : "warn"} />
            <span className="flex-1 font-medium">{name}</span>
            <span className={`font-semibold tabular-nums ${scoreColor(score)}`}>{score}</span>
          </div>
        ))}
      </div>
      <div className="flex h-8 items-center justify-end gap-1 border-t border-line bg-panel px-2">
        <span className="rounded-md px-2 py-0.5 text-dim">Expand</span>
        <span className="rounded-md bg-accent px-2 py-0.5 font-semibold text-on-accent">Get Up To Speed</span>
      </div>
    </div>
  );
}
