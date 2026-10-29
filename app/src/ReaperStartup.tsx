import { useCallback, useEffect, useState } from "react";
import { bringFiles, reaperStartup, type StartupAction, type StartupCheck } from "./api";
import { bringSummary } from "./bring";
import { Button, Notice } from "./ui";

const ROOT = "REAPER_RESOURCES";
const STARTUP_SCRIPT = "Scripts/__startup.lua";

/** "Custom: JMD_Startup", or its id when the key map doesn't name it. */
function named(a: StartupAction): string {
  if (a.name) return a.name;
  return a.kind === "native" ? `Reaper action ${a.id}` : a.id;
}

const scripts = (n: number) => `${n} script${n === 1 ? "" : "s"}`;

/** The line, and the files its fix brings over from the Primary Rig (none on the Primary Rig itself). */
function what(c: StartupCheck): { text: string; detail: string | null; keys: string[]; fix: string } | null {
  // A custom action this machine doesn't have comes with the Primary Rig's key map.
  const sws = [`${ROOT}/S&M.ini`, ...(c.primary_unknown ? [`${ROOT}/reaper-kb.ini`] : [])];
  const theirs = c.primary ? named(c.primary) : null;
  const viaKeyMap = c.primary_unknown
    ? "It's a custom action this machine doesn't have yet, so your key map comes over with it."
    : null;
  switch (c.status) {
    case "not_set":
      return {
        text: theirs
          ? `Reaper here doesn't run ${theirs} when it opens, as the Primary Rig does.`
          : "Reaper here doesn't run the Primary Rig's startup action when it opens.",
        detail: viaKeyMap,
        keys: sws,
        fix: "Set it",
      };
    case "different":
      return {
        text: `Reaper here runs ${named(c.here!)} when it opens; the Primary Rig runs ${theirs ?? "another action"}.`,
        detail: viaKeyMap,
        keys: sws,
        fix: "Use the Primary Rig's",
      };
    case "not_found":
      return {
        text: `Reaper's startup action here (${c.here!.id}) isn't one of this machine's actions, so it doesn't run.`,
        detail: c.has_primary
          ? "Set another in Extensions > Startup actions, or bring over the Primary Rig's key map."
          : "Set another in Extensions > Startup actions.",
        keys: c.has_primary ? [`${ROOT}/reaper-kb.ini`] : [],
        fix: "Bring over key map",
      };
    case "ok":
      break;
  }
  if (c.missing_scripts.length > 0 && c.here) {
    return {
      text: `Reaper's startup action, ${named(c.here)}, runs ${scripts(c.missing_scripts.length)} this machine doesn't have.`,
      detail: c.missing_scripts.join("\n"),
      keys: c.has_primary ? c.missing_scripts.map((rel) => `${ROOT}/${rel}`) : [],
      fix: "Bring them over",
    };
  }
  if (c.startup_script_differs) {
    return {
      text: "The Primary Rig's startup script (__startup.lua) isn't the same here.",
      detail: null,
      keys: [`${ROOT}/${STARTUP_SCRIPT}`],
      fix: "Bring it over",
    };
  }
  return null;
}

/**
 * Reaper's startup action (decision #107): one Notice line when the action
 * SWS runs as Reaper opens here isn't the Primary Rig's, can't run here, or
 * runs scripts this machine lacks. Its fix brings the Primary Rig's over
 * (only that line of S&M.ini; Reaper closed, undo in History). Shows nothing
 * when all is well. Rechecked when the window comes back to the front.
 */
export function ReaperStartupNotice({ onError }: { onError: (e: string) => void }) {
  const [check, setCheck] = useState<StartupCheck | null>(null);
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const [done, setDone] = useState<string | null>(null);

  const load = useCallback(() => {
    reaperStartup().then(setCheck, (e) => onError(String(e)));
  }, [onError]);
  useEffect(() => {
    load();
    window.addEventListener("focus", load);
    return () => window.removeEventListener("focus", load);
  }, [load]);

  if (!check) return null;
  const line = what(check);
  if (!line) return done ? <Notice tone="ok" text={<span className="text-ok">{done}</span>} /> : null;

  async function fix(keys: string[]) {
    setBusy(true);
    setDone(null);
    try {
      const r = await bringFiles(keys);
      const { text, warn } = bringSummary(r, "the Primary Rig");
      setDone(r.written > 0 ? `${text} Reopen Reaper to run it.` : (warn ?? text));
      load();
    } catch (e) {
      onError(String(e));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Notice
      tone="warn"
      open={open}
      onToggle={() => setOpen((o) => !o)}
      text={<span className="font-medium">{line.text}</span>}
      actions={
        line.keys.length > 0 && (
          <Button
            variant="link"
            onClick={() => fix(line.keys)}
            disabled={busy}
            title="Reaper must be closed. Undo in History."
            className="h-6 shrink-0 whitespace-nowrap px-2"
          >
            {busy ? "Bringing over…" : line.fix}
          </Button>
        )
      }
    >
      <div className="flex flex-col gap-1 pb-1.5 pl-6 pr-2 pt-0.5 text-[11px] text-dim">
        {done && <p className="text-ok">{done}</p>}
        {line.detail && <p className="whitespace-pre-line">{line.detail}</p>}
        <p>SWS runs it each time Reaper opens (Extensions &gt; Startup actions). Reaper must be closed to change it.</p>
      </div>
    </Notice>
  );
}
