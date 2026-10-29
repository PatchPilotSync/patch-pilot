import { useState } from "react";
import { Button, inputClass, Mark, Notice, Panel } from "../ui";

export function Welcome({ onScan, onTour }: { onScan: (name: string) => void; onTour: () => void }) {
  const [name, setName] = useState("");
  return (
    <div className="mx-auto flex max-w-md flex-col gap-4 px-6 pt-[14vh]">
      <div>
        <AppMark />
        <h1 className="text-lg font-semibold">Let's see what's on this machine</h1>
        <p className="mt-1 leading-relaxed text-dim">
          Patch Pilot keeps every machine in your rig matching.{" "}
          <button type="button" onClick={onTour} className="text-accent-text hover:underline">
            How it works
          </button>
        </p>
      </div>
      <Panel className="p-1">
        <Notice icon="plug" text="Lists your plug-ins, vendor apps and Reaper templates" />
        <Notice icon="sync" text="Keeps that list current on its own" />
        <Notice tone="ok" icon="check" text="Read only: nothing is loaded, changed or uploaded" />
      </Panel>
      <form
        className="flex flex-col gap-2"
        onSubmit={(e) => {
          e.preventDefault();
          onScan(name);
        }}
      >
        <label className="text-xs text-dim" htmlFor="machine-name">
          Name this machine
        </label>
        <input
          id="machine-name"
          autoFocus
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder="Studio PC"
          className={inputClass}
        />
        <Button type="submit" className="mt-1 self-start">
          Scan this machine
        </Button>
      </form>
    </div>
  );
}

/** The logo in a small framed square, over the first-run screens' titles. */
export function AppMark() {
  return (
    <span className="mb-3 flex size-10 items-center justify-center rounded-lg border border-line bg-panel">
      <Mark size={22} className="text-accent-text" />
    </span>
  );
}
