import type { Progress } from "../api";

const STEPS = [
  "Checking vendor apps and licensing tools",
  "Reading plug-in folders",
  "Reading Reaper templates",
  "Saving the snapshot",
];

export function Scanning({ progress }: { progress: Progress | null }) {
  const current = progress?.step ?? 0;
  return (
    <div className="mx-auto flex max-w-sm flex-col gap-4 px-6 pt-[16vh]">
      <h1 className="text-lg font-semibold">Scanning…</h1>
      <div className="h-[3px] overflow-hidden rounded-full bg-track">
        <div
          className="h-full rounded-full bg-accent transition-all duration-300"
          style={{ width: `${(Math.max(current - 0.5, 0) / STEPS.length) * 100}%` }}
        />
      </div>
      <ol className="flex flex-col gap-1.5">
        {STEPS.map((label, i) => {
          const step = i + 1;
          const state = step < current ? "done" : step === current ? "now" : "next";
          return (
            <li
              key={label}
              className={
                state === "next" ? "text-dim opacity-60" : state === "now" ? "font-medium" : "text-dim"
              }
            >
              <span className="mr-2 inline-block w-4 text-center">
                {state === "done" ? "✓" : state === "now" ? "•" : ""}
              </span>
              {label}
            </li>
          );
        })}
      </ol>
    </div>
  );
}
