import { useState } from "react";
import { Button } from "../ui";

export function Welcome({ onScan }: { onScan: (name: string) => void }) {
  const [name, setName] = useState("");
  return (
    <div className="mx-auto flex max-w-md flex-col gap-6 pt-[18vh]">
      <div>
        <h1 className="text-2xl font-semibold tracking-tight">Let's see what's on this machine</h1>
        <p className="mt-2 text-sm text-neutral-500">
          Patch Pilot reads your plug-in folders, vendor apps and Reaper templates. It only reads
          metadata: nothing is loaded, changed or uploaded.
        </p>
      </div>
      <form
        className="flex flex-col gap-3"
        onSubmit={(e) => {
          e.preventDefault();
          onScan(name);
        }}
      >
        <label className="text-xs font-medium text-neutral-500" htmlFor="machine-name">
          Name this machine
        </label>
        <input
          id="machine-name"
          autoFocus
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder="Studio PC"
          className="rounded-md border border-neutral-300 bg-transparent px-3 py-2 text-sm outline-none focus:border-neutral-900 dark:border-neutral-700 dark:focus:border-neutral-300"
        />
        <Button type="submit" className="self-start">
          Scan this machine
        </Button>
      </form>
    </div>
  );
}
