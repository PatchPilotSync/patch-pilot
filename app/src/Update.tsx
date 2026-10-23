import { useEffect, useState, type ReactNode } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { Button } from "./ui";

// Self-update (decision #32). One check at launch, never a timer, so the app
// stays idle. Nothing downloads or installs until the user clicks.

/** Notes of the update being installed, shown once after the restart. */
const JUST_INSTALLED = "patchpilot.just-installed";

type State =
  | { kind: "idle" }
  | { kind: "available"; update: Update; open: boolean }
  | { kind: "installing"; update: Update; done: number; total: number | null }
  | { kind: "failed"; update: Update; error: string }
  | { kind: "installed"; version: string; notes: string };

export function UpdateBar() {
  const [state, setState] = useState<State>({ kind: "idle" });

  useEffect(() => {
    let cancelled = false;
    (async () => {
      const installed = await justInstalled();
      if (cancelled) return;
      if (installed) {
        setState({ kind: "installed", ...installed });
        return;
      }
      // Dev builds have no release to compare against.
      if (import.meta.env.DEV) return;
      try {
        const update = await check();
        if (update && !cancelled) setState({ kind: "available", update, open: false });
      } catch (e) {
        // Offline or no release yet: not worth interrupting anyone for.
        console.warn("update check failed", e);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  async function install(update: Update) {
    setState({ kind: "installing", update, done: 0, total: null });
    try {
      remember({ version: update.version, notes: update.body ?? "" });
      let done = 0;
      let total: number | null = null;
      await update.downloadAndInstall((e) => {
        if (e.event === "Started") total = e.data.contentLength ?? null;
        if (e.event === "Progress") done += e.data.chunkLength;
        setState({ kind: "installing", update, done, total });
      });
      // Windows exits inside downloadAndInstall to run the installer.
      await relaunch();
    } catch (e) {
      remember(null);
      setState({ kind: "failed", update, error: String(e) });
    }
  }

  if (state.kind === "idle") return null;

  if (state.kind === "installed") {
    return (
      <Bar>
        <div className="flex items-center gap-3">
          <span className="flex-1">
            Updated to <strong>Patch Pilot {state.version}</strong>
          </span>
          <Button variant="quiet" onClick={() => setState({ kind: "idle" })}>
            Dismiss
          </Button>
        </div>
        {state.notes && <Notes text={state.notes} />}
      </Bar>
    );
  }

  const { update } = state;
  return (
    <Bar>
      <div className="flex items-center gap-3">
        <span className="flex-1">
          <strong>Patch Pilot {update.version}</strong> is available. You have {update.currentVersion}.
        </span>
        {state.kind === "available" && (
          <>
            {update.body && (
              <Button variant="quiet" onClick={() => setState({ ...state, open: !state.open })}>
                {state.open ? "Hide changes" : "What's new"}
              </Button>
            )}
            <Button variant="quiet" onClick={() => setState({ kind: "idle" })}>
              Later
            </Button>
            <Button onClick={() => install(update)}>Install and restart</Button>
          </>
        )}
        {state.kind === "installing" && (
          <span className="tabular-nums text-dim">
            {state.total ? `Downloading ${Math.floor((100 * state.done) / state.total)}%` : "Downloading…"}
          </span>
        )}
        {state.kind === "failed" && (
          <Button onClick={() => install(update)}>Try again</Button>
        )}
      </div>
      {state.kind === "failed" && (
        <p className="mt-1 text-bad">The update didn't install: {state.error}</p>
      )}
      {((state.kind === "available" && state.open) || state.kind === "installing") && update.body && (
        <Notes text={update.body} />
      )}
    </Bar>
  );
}

function Bar({ children }: { children: ReactNode }) {
  return (
    <div className="shrink-0 border-b border-line bg-accent/10 px-4 py-1.5">
      {children}
    </div>
  );
}

/** Release notes are the version's CHANGELOG.md section: headings and bullets. */
export function Notes({ text }: { text: string }) {
  return (
    <div className="mt-2 max-h-64 space-y-1 overflow-y-auto rounded-md border border-line bg-panel px-3 py-2 leading-relaxed">
      {parseNotes(text).map((b, i) =>
        b.kind === "heading" ? (
          <h4 key={i} className="pt-1 font-semibold">
            {b.text}
          </h4>
        ) : b.kind === "bullets" ? (
          <ul key={i} className="list-disc pl-5">
            {b.items.map((t, j) => (
              <li key={j}>{t}</li>
            ))}
          </ul>
        ) : (
          <p key={i}>{b.text}</p>
        ),
      )}
    </div>
  );
}

export type NoteBlock =
  | { kind: "heading"; text: string }
  | { kind: "bullets"; items: string[] }
  | { kind: "text"; text: string };

export function parseNotes(text: string): NoteBlock[] {
  const blocks: NoteBlock[] = [];
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.trim().replace(/\*\*(.+?)\*\*/g, "$1").replace(/`(.+?)`/g, "$1");
    if (!line) continue;
    const heading = line.match(/^#{1,6}\s+(.*)$/);
    const bullet = line.match(/^[-*]\s+(.*)$/);
    const last = blocks[blocks.length - 1];
    if (heading) blocks.push({ kind: "heading", text: heading[1] });
    else if (bullet && last?.kind === "bullets") last.items.push(bullet[1]);
    else if (bullet) blocks.push({ kind: "bullets", items: [bullet[1]] });
    else blocks.push({ kind: "text", text: line });
  }
  return blocks;
}

function remember(v: { version: string; notes: string } | null) {
  try {
    if (v) localStorage.setItem(JUST_INSTALLED, JSON.stringify(v));
    else localStorage.removeItem(JUST_INSTALLED);
  } catch {
    // Storage off: the "what's new" note after restart is a nicety.
  }
}

/** The notes saved before the restart, if this launch is that version. */
async function justInstalled(): Promise<{ version: string; notes: string } | null> {
  let saved: { version: string; notes: string } | null = null;
  try {
    const raw = localStorage.getItem(JUST_INSTALLED);
    saved = raw ? JSON.parse(raw) : null;
  } catch {
    return null;
  }
  if (!saved) return null;
  remember(null);
  return saved.version === (await getVersion()) ? saved : null;
}
