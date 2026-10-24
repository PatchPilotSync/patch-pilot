import { useEffect, useRef, useState, type ReactNode } from "react";
import { Badge, Button, Mark, ScoreBar, scoreColor } from "./ui";

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
    body: "Patch Pilot keeps your studio, laptop and travel machines in step, so a project that opens on one opens on all of them. It only reads what's installed: nothing is loaded, changed or copied.",
    art: <MachinesArt />,
  },
  {
    title: "The Golden Rig",
    body: "Your ideal setup: the plug-ins, versions, vendor apps and templates every machine should have. Make it from your best machine, and update it whenever your setup changes. Each version is kept.",
    art: <GoldenArt />,
  },
  {
    title: "Compare and catch drift",
    body: "Compare shows what each machine is missing next to the Golden Rig. Anything missing, older or short a format is drift, and the parity score sums it up, always with the reasons behind it.",
    art: <CompareArt />,
  },
  {
    title: "Get up to speed",
    body: "Up To Speed turns the differences into a checklist, starting with what other installs depend on. You install things yourself; Patch Pilot never runs an installer or touches a license.",
    art: <ChecklistArt />,
  },
  {
    title: `Lives in your ${TRAY}`,
    body: (
      <>
        Click the plane in the {TRAY} for every machine's score. Expand, or double-click the popover, to open this
        window. It opens when your computer starts and rescans on its own when plug-in folders change; each change is
        saved as a snapshot.
        <span className="mt-2 flex items-center gap-1.5">
          <Badge>Soon</Badge>
          <span>Installer Vault: your installers, on your drive.</span>
        </span>
      </>
    ),
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

// Illustrations. Small, token-coloured, decorative only.

function Chip({ children, className = "" }: { children: ReactNode; className?: string }) {
  return (
    <div className={`flex h-7 items-center gap-2 rounded-md border border-line bg-panel px-2.5 text-xs ${className}`}>
      {children}
    </div>
  );
}

function Dot({ tone }: { tone: "ok" | "warn" | "bad" | "dim" }) {
  const fill = { ok: "bg-ok", warn: "bg-warn", bad: "bg-bad", dim: "bg-dim" }[tone];
  return <span className={`h-1.5 w-1.5 shrink-0 rounded-full ${fill}`} />;
}

function MachinesArt() {
  return (
    <div className="flex items-center gap-4" aria-hidden="true">
      <div className="flex flex-col gap-1.5">
        <Chip>
          <Dot tone="ok" /> Studio PC
        </Chip>
        <Chip>
          <Dot tone="ok" /> MacBook
        </Chip>
        <Chip>
          <Dot tone="warn" /> Travel laptop
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
      <div className="flex items-center gap-1.5 px-2.5 py-1">
        <span className="text-warn">★</span>
        <span className="font-semibold">Golden Rig</span>
        <span className="flex-1" />
        <Badge>v3</Badge>
      </div>
      {[
        ["Pro-Q 4", "4.0.2"],
        ["Serum 2", "2.0.14"],
        ["Kontakt 8", "8.4.1"],
      ].map(([name, v]) => (
        <div key={name} className="flex h-6 items-center gap-2 px-2.5">
          <span className="text-ok">✓</span>
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
      <div className="w-[76px] text-center">
        <div className="text-[28px] font-semibold leading-none text-warn tabular-nums">87</div>
        <ScoreBar score={87} className="mt-2" />
        <div className="mt-1.5 text-[11px] text-dim">parity</div>
      </div>
      <div className="w-[170px] rounded-md border border-line bg-panel py-1 text-xs">
        {(
          [
            ["bad", "Serum 2", "Missing"],
            ["warn", "Pro-Q 4", "Outdated"],
            ["warn", "Kontakt 8", "No AAX"],
          ] as const
        ).map(([tone, name, note]) => (
          <div key={name} className="flex h-6 items-center gap-2 px-2.5">
            <Dot tone={tone} />
            <span className="flex-1">{name}</span>
            <span className="text-[11px] text-dim">{note}</span>
          </div>
        ))}
      </div>
    </div>
  );
}

function ChecklistArt() {
  return (
    <div className="w-[220px] rounded-md border border-line bg-panel py-1 text-xs" aria-hidden="true">
      <div className="px-2.5 py-1 text-[11px] text-dim">Travel laptop needs</div>
      {(
        [
          [true, "iLok License Manager"],
          [true, "Serum 2"],
          [false, "Update Pro-Q 4 to 4.0.2"],
        ] as const
      ).map(([done, label]) => (
        <div key={label} className="flex h-6 items-center gap-2 px-2.5">
          <span
            className={`flex h-3.5 w-3.5 shrink-0 items-center justify-center rounded-[4px] text-[9px] ${
              done ? "bg-accent text-on-accent" : "border border-dim"
            }`}
          >
            {done ? "✓" : ""}
          </span>
          <span className={done ? "text-dim line-through" : ""}>{label}</span>
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
    <div className="w-[230px] rounded-md border border-line bg-panel text-xs shadow-sm" aria-hidden="true">
      <div className="flex h-7 items-center gap-1.5 px-2.5">
        <Mark size={12} className="text-accent-text" />
        <span className="font-semibold">Patch Pilot</span>
        <span className="flex-1" />
        <span className="text-[10px] text-dim">Scanned just now</span>
      </div>
      <div className="flex flex-col gap-1 px-2 pb-2">
        {rows.map(([name, score]) => (
          <div key={name} className="flex h-7 items-center gap-2 rounded-md border border-line bg-canvas px-2.5">
            <span className="flex-1 font-medium">{name}</span>
            <span className={`font-semibold tabular-nums ${scoreColor(score)}`}>{score}</span>
          </div>
        ))}
      </div>
      <div className="flex h-8 items-center justify-end gap-1 border-t border-line px-2">
        <span className="rounded-md px-2 py-0.5 text-dim">Expand</span>
        <span className="rounded-md bg-accent px-2 py-0.5 font-semibold text-on-accent">Up To Speed</span>
      </div>
    </div>
  );
}
