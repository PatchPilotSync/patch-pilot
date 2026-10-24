import { useState } from "react";
import { Button, inputClass } from "../ui";

export function Welcome({ onScan, onTour }: { onScan: (name: string) => void; onTour: () => void }) {
  const [name, setName] = useState("");
  return (
    <div className="mx-auto flex max-w-sm flex-col gap-5 px-6 pt-[16vh]">
      <div>
        <h1 className="text-lg font-semibold">Let's see what's on this machine</h1>
        <p className="mt-1.5 leading-relaxed text-dim">
          Patch Pilot lists the plug-ins, vendor apps and Reaper templates installed here, so you can keep
          every machine matching. It only reads metadata: nothing is loaded, changed or uploaded.{" "}
          <button type="button" onClick={onTour} className="text-accent-text hover:underline">
            How it works
          </button>
        </p>
      </div>
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
