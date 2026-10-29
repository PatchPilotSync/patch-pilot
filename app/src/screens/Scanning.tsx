import type { Progress } from "../api";
import { Panel, Tile, type GlyphName } from "../ui";
import { AppMark } from "./Welcome";

const STEPS: [label: string, glyph: GlyphName][] = [
  ["Checking vendor apps and licensing tools", "apps"],
  ["Reading plug-in folders", "plug"],
  ["Reading Reaper templates", "doc"],
  ["Saving the snapshot", "data"],
];

/** `updated`: the rescan on the first launch after an update. */
export function Scanning({ progress, updated = false }: { progress: Progress | null; updated?: boolean }) {
  const current = progress?.step ?? 0;
  return (
    <div className="mx-auto flex max-w-md flex-col gap-4 px-6 pt-[14vh]">
      <div>
        <AppMark />
        <h1 className="text-lg font-semibold">Scanning this machine…</h1>
        <p className="mt-1 text-dim">
          {updated
            ? "Patch Pilot was updated, so it's reading what's installed again. Nothing is changed."
            : "Reading what's installed. Nothing is changed."}
        </p>
      </div>
      <Panel>
        <div className="h-[3px] bg-track">
          <div
            className="h-full bg-accent transition-all duration-300"
            style={{ width: `${(Math.max(current - 0.5, 0) / STEPS.length) * 100}%` }}
          />
        </div>
        <ol className="flex flex-col divide-y divide-line p-1 pt-0.5">
          {STEPS.map(([label, glyph], i) => {
            const step = i + 1;
            const state = step < current ? "done" : step === current ? "now" : "next";
            return (
              <li key={label} className="flex h-9 items-center gap-2.5 px-2 text-xs">
                <Tile
                  name={state === "done" ? "check" : glyph}
                  tone={state === "done" ? "ok" : state === "now" ? "accent" : "dim"}
                />
                <span className={state === "now" ? "font-medium" : "text-dim"}>{label}</span>
              </li>
            );
          })}
        </ol>
      </Panel>
    </div>
  );
}
