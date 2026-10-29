import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { listen } from "@tauri-apps/api/event";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { settingsView } from "./api";
import { Banner, Button } from "./ui";

// Self-update (decision #32). Checks at launch and again when the window is
// shown or focused, at most once an hour. No timer, so the app stays idle
// in the tray. "Check for Updates…" in the tray menu checks right away and
// says so either way. Nothing downloads or installs until the user clicks.

/** The app stays running from login, so launch alone is too rare a check. */
const RECHECK_MS = 60 * 60 * 1000;

/** Notes of the update being installed, shown once after the restart. */
const JUST_INSTALLED = "patchpilot.just-installed";

type State =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "current"; version: string }
  | { kind: "check-failed"; error: string }
  | { kind: "available"; update: Update; open: boolean }
  | { kind: "installing"; update: Update; done: number; total: number | null }
  | { kind: "failed"; update: Update; error: string }
  | { kind: "installed"; version: string; notes: string };

export function UpdateBar() {
  const [state, setState] = useState<State>({ kind: "idle" });

  useEffect(() => {
    let cancelled = false;
    let lastCheck = 0;
    async function checkNow() {
      // Dev builds have no release to compare against.
      if (import.meta.env.DEV) return;
      lastCheck = Date.now();
      // Settings can turn the offers off (decision #97); Check for Updates still asks.
      const offers = await settingsView().then(
        (v) => v.prefs.update_notices,
        () => true,
      );
      if (!offers) return;
      try {
        const update = await check();
        // Keep whatever the bar is already showing (an install in progress,
        // or an offer the user is reading).
        if (update && !cancelled)
          setState((s) => (s.kind === "idle" ? { kind: "available", update, open: false } : s));
      } catch (e) {
        // Offline or no release yet: not worth interrupting anyone for.
        console.warn("update check failed", e);
      }
    }
    // Asked for from the tray menu: always check, and answer even when
    // there's nothing new.
    async function checkAsked() {
      lastCheck = Date.now();
      setState((s) => (s.kind === "installing" ? s : { kind: "checking" }));
      try {
        const update = await check();
        if (cancelled) return;
        if (update) setState({ kind: "available", update, open: false });
        else setState({ kind: "current", version: await getVersion() });
      } catch (e) {
        if (!cancelled) setState({ kind: "check-failed", error: String(e) });
      }
    }
    function onShown() {
      if (document.visibilityState === "visible" && Date.now() - lastCheck > RECHECK_MS) checkNow();
    }
    (async () => {
      const installed = await justInstalled();
      if (cancelled) return;
      if (installed) setState({ kind: "installed", ...installed });
      else checkNow();
    })();
    const unlistenAsked = listen("check-for-updates", checkAsked);
    window.addEventListener("focus", onShown);
    document.addEventListener("visibilitychange", onShown);
    return () => {
      cancelled = true;
      unlistenAsked.then((f) => f());
      window.removeEventListener("focus", onShown);
      document.removeEventListener("visibilitychange", onShown);
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

  if (state.kind === "checking" || state.kind === "current" || state.kind === "check-failed") {
    return (
      <Banner
        icon={state.kind === "check-failed" ? "info" : state.kind === "current" ? "check" : "sync"}
        tone={state.kind === "check-failed" ? "bad" : state.kind === "current" ? "ok" : "accent"}
        text={
          <span className={state.kind === "check-failed" ? "text-bad" : ""}>
            {state.kind === "checking" && "Checking for updates…"}
            {state.kind === "current" && (
              <>
                You're up to date with <strong>Patch Pilot {state.version}</strong>.
              </>
            )}
            {state.kind === "check-failed" && `Couldn't check for updates: ${state.error}`}
          </span>
        }
        actions={
          state.kind !== "checking" && (
            <Button variant="quiet" onClick={() => setState({ kind: "idle" })}>
              Dismiss
            </Button>
          )
        }
      />
    );
  }

  if (state.kind === "installed") {
    return (
      <Banner
        icon="check"
        tone="ok"
        text={
          <>
            Updated to <strong>Patch Pilot {state.version}</strong>
          </>
        }
        actions={
          <Button variant="quiet" onClick={() => setState({ kind: "idle" })}>
            Dismiss
          </Button>
        }
      >
        {state.notes && <Notes text={state.notes} />}
      </Banner>
    );
  }

  const { update } = state;
  const notes = ((state.kind === "available" && state.open) || state.kind === "installing") && update.body;
  return (
    <Banner
      icon={state.kind === "failed" ? "info" : "download"}
      tone={state.kind === "failed" ? "bad" : "accent"}
      text={
        <>
          <strong>Patch Pilot {update.version}</strong> is available. You have {update.currentVersion}.
          {state.kind === "failed" && <span className="block text-bad">The update didn't install: {state.error}</span>}
        </>
      }
      actions={
        <>
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
            <span className="px-2 tabular-nums text-dim">
              {state.total ? `Downloading ${Math.floor((100 * state.done) / state.total)}%` : "Downloading…"}
            </span>
          )}
          {state.kind === "failed" && <Button onClick={() => install(update)}>Try again</Button>}
        </>
      }
    >
      {notes && <Notes text={notes} />}
    </Banner>
  );
}

/** Release notes are the version's CHANGELOG.md section: headings and bullets. */
export function Notes({ text }: { text: string }) {
  return (
    <div className="mt-2 max-h-64 space-y-1 overflow-y-auto rounded-md border border-line bg-panel px-3 py-2 leading-relaxed in-[.pp-panel]:bg-canvas">
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
