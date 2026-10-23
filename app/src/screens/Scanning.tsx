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
    <div className="mx-auto flex max-w-md flex-col gap-6 pt-[18vh]">
      <h1 className="text-2xl font-semibold tracking-tight">Scanning…</h1>
      <div className="h-1 overflow-hidden rounded bg-neutral-200 dark:bg-neutral-800">
        <div
          className="h-full bg-emerald-500 transition-all duration-300"
          style={{ width: `${(Math.max(current - 0.5, 0) / STEPS.length) * 100}%` }}
        />
      </div>
      <ol className="flex flex-col gap-2 text-sm">
        {STEPS.map((label, i) => {
          const step = i + 1;
          const state = step < current ? "done" : step === current ? "now" : "next";
          return (
            <li
              key={label}
              className={
                state === "next"
                  ? "text-neutral-400 dark:text-neutral-600"
                  : state === "now"
                    ? "font-medium"
                    : "text-neutral-500"
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
