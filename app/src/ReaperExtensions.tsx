import { useCallback, useEffect, useState } from "react";
import {
  fixReaperExtension,
  openWebPage,
  reaperExtensions,
  revealPath,
  SHOW_IN_FOLDER,
  type ExtensionCheck,
  type ExtensionFixAction,
  type ExtensionState,
} from "./api";
import { Badge, Button, Notice } from "./ui";

const IS_MAC = /Mac/i.test(navigator.userAgent);

/** The fix buttons' labels. */
const FIX_LABEL: Record<ExtensionFixAction, string> = {
  unblock: "Unblock",
  move_up: "Move into UserPlugins",
  remove: "Remove older copy",
};

/** What each fix leaves to do, said once it's done. */
function doneText(action: ExtensionFixAction, name: string): string {
  switch (action) {
    case "unblock":
      return `Unblocked. Quit and reopen Reaper so it loads ${name}.`;
    case "move_up":
      return `Moved. Reopen Reaper so it loads ${name}. Undo in History.`;
    case "remove":
      return `Removed the older copy of ${name}. Undo in History.`;
  }
}

/** One line on what's wrong with one extension. */
function headline(e: ExtensionState): string {
  switch (e.status) {
    case "blocked":
      return `${e.name} won't load in Reaper: macOS blocked it.`;
    case "in_subfolder":
      return `${e.name} is in a subfolder Reaper doesn't look in.`;
    case "wrong_cpu":
      return `${e.name} is the wrong build for this ${IS_MAC ? "Mac" : "PC"}.`;
    case "wrong_os":
      return `${e.name} is the ${IS_MAC ? "Windows" : "Mac"} build.`;
    case "damaged":
      return `${e.name}'s file is incomplete.`;
    case "missing":
      return `Scripts here need ${e.name}, which isn't installed.`;
    case "two_copies":
      return `${e.name} is in UserPlugins twice.`;
    case "ok":
      return `${e.name} loads.`;
  }
}

const STATUS_BADGE: Record<ExtensionState["status"], string> = {
  blocked: "Blocked by macOS",
  in_subfolder: "In a subfolder",
  wrong_cpu: "Wrong build",
  wrong_os: "Wrong build",
  damaged: "Incomplete",
  missing: "Not installed",
  two_copies: "Two copies",
  ok: "Loads",
};

/** "nvk_FOLDER_ITEMS - Reposition.lua and 33 more scripts", by file name. */
function usedBy(scripts: string[]): string {
  const name = (s: string) => s.split("/").pop() ?? s;
  if (scripts.length === 0) return "";
  if (scripts.length === 1) return `Used by ${name(scripts[0])}`;
  return `Used by ${name(scripts[0])} and ${scripts.length - 1} more script${scripts.length === 2 ? "" : "s"}`;
}

/**
 * Reaper extensions (decision #104): one Notice line when an extension here
 * won't load (blocked by macOS, in a subfolder, the wrong build, incomplete,
 * there twice) or scripts here call one that isn't installed. It opens to
 * each one with its fix or where to get this machine's build. Shows nothing
 * while every extension loads. Loads its own data, and again when the window
 * comes back to the front (after a download, say).
 */
export function ReaperExtensionsNotice({ onError }: { onError: (e: string) => void }) {
  const [check, setCheck] = useState<ExtensionCheck | null>(null);
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const [done, setDone] = useState<string | null>(null);

  const load = useCallback(() => {
    reaperExtensions().then(setCheck, (e) => onError(String(e)));
  }, [onError]);
  useEffect(() => {
    load();
    window.addEventListener("focus", load);
    return () => window.removeEventListener("focus", load);
  }, [load]);

  if (!check) return null;
  const problems = check.extensions.filter((e) => e.status !== "ok");
  if (problems.length === 0 && !done) return null;

  async function fix(e: ExtensionState) {
    if (!e.fix) return;
    const { action, file } = e.fix;
    setBusy(file);
    setDone(null);
    try {
      setCheck(await fixReaperExtension(file, action));
      setDone(doneText(action, e.name));
    } catch (err) {
      onError(String(err));
    } finally {
      setBusy(null);
    }
  }

  const sep = IS_MAC ? "/" : "\\";
  const reveal = (e: ExtensionState) =>
    revealPath(e.file ? `${check.folder}${sep}${e.file.split("/").join(sep)}` : check.folder).catch(() =>
      revealPath(check.folder).catch((err) => onError(String(err))),
    );
  const only = problems.length === 1 ? problems[0] : null;

  if (problems.length === 0) {
    return <Notice tone="ok" text={<span className="text-ok">{done}</span>} />;
  }
  return (
    <Notice
      tone="warn"
      open={open}
      onToggle={() => setOpen((o) => !o)}
      text={
        <>
          <span className="font-medium">
            {only ? headline(only) : `${problems.length} Reaper extensions need attention.`}
          </span>
          {only?.needed_by.length ? <span className="text-dim"> {usedBy(only.needed_by)}.</span> : null}
        </>
      }
      actions={
        only?.fix && (
          <Button
            variant="link"
            onClick={() => fix(only)}
            disabled={busy !== null}
            className="h-6 shrink-0 whitespace-nowrap px-2"
          >
            {busy ? "Fixing…" : FIX_LABEL[only.fix.action]}
          </Button>
        )
      }
    >
      <ul className="flex flex-col gap-2 pb-1.5 pl-6 pr-2 pt-0.5">
        {done && <li className="text-[11px] text-ok">{done}</li>}
        {problems.map((e) => (
          <li key={e.product_id} className="flex flex-col gap-0.5">
            <div className="flex items-center gap-2">
              <span className="min-w-0 flex-1 truncate font-medium">{e.name}</span>
              <Badge tone="amber">{STATUS_BADGE[e.status]}</Badge>
            </div>
            <p className="text-[11px] text-dim">{e.detail}</p>
            {e.file && (
              <p className="truncate text-[11px] text-dim opacity-80" title={e.file}>
                UserPlugins/{e.file}
              </p>
            )}
            {e.needed_by.length > 0 && (
              <p className="truncate text-[11px] text-dim opacity-80" title={e.needed_by.join("\n")}>
                {usedBy(e.needed_by)}
              </p>
            )}
            <div className="-ml-2 flex flex-wrap items-center">
              {e.fix && (
                <Button
                  variant="link"
                  onClick={() => fix(e)}
                  disabled={busy !== null}
                  className="h-6 whitespace-nowrap px-2"
                >
                  {busy === e.fix.file ? "Fixing…" : FIX_LABEL[e.fix.action]}
                </Button>
              )}
              {e.download && !e.fix && (
                <Button
                  variant="link"
                  onClick={() => openWebPage(e.download!.page).catch((err) => onError(String(err)))}
                  title={`Opens ${e.download.page}`}
                  className="h-6 whitespace-nowrap px-2"
                >
                  {e.download.file ? `Get ${e.download.file}` : "Downloads"}
                </Button>
              )}
              <Button variant="quiet" onClick={() => reveal(e)} className="h-6 whitespace-nowrap px-2">
                {SHOW_IN_FOLDER}
              </Button>
            </div>
          </li>
        ))}
        <li>
          <button onClick={load} className="text-[11px] text-dim hover:text-fg">
            Check again
          </button>
        </li>
      </ul>
    </Notice>
  );
}
