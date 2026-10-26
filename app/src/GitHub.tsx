import { useCallback, useEffect, useState } from "react";
import {
  githubCancelSignIn,
  githubOpen,
  githubSignIn,
  githubSignOut,
  githubUseRepo,
  githubUseToken,
  githubView,
  onGitHubChanged,
  type GitHubView,
} from "./api";
import { Button, inputClass } from "./ui";

/**
 * Advanced: sync with GitHub (decision #72), in place of the cloud folder
 * picker. Sign in once, then pick or name a private repo; backups and
 * keeping files in sync go through it from then on. `onUsed` runs once the
 * repo is in use; `onCancel` goes back to the cloud folders.
 */
export function GitHubSetup({
  onUsed,
  onCancel,
  onError,
}: {
  onUsed: () => void;
  onCancel: () => void;
  onError: (e: string) => void;
}) {
  const [view, setView] = useState<GitHubView | null>(null);
  const [busy, setBusy] = useState(false);
  const [token, setToken] = useState("");
  const [repo, setRepo] = useState<string | null>(null);

  const load = useCallback(() => {
    githubView().then(setView, (e) => onError(String(e)));
  }, [onError]);
  useEffect(() => {
    load();
    const unlisten = onGitHubChanged(load);
    return () => {
      unlisten.then((f) => f());
    };
  }, [load]);

  if (!view) return null;

  async function act(p: Promise<GitHubView>, then?: () => void) {
    setBusy(true);
    try {
      setView(await p);
      then?.();
    } catch (e) {
      onError(String(e));
    } finally {
      setBusy(false);
    }
  }

  const name = repo ?? view.repo ?? view.default_repo;
  const cancel = (
    <Button variant="quiet" disabled={busy} onClick={onCancel}>
      Cancel
    </Button>
  );

  return (
    <div className="flex flex-col gap-1.5 text-xs">
      <p className="text-dim">
        Sync through a private GitHub repo instead of a cloud folder. Every change stays in the repo&apos;s history. Big
        files still need a cloud folder.
      </p>
      {!view.login ? (
        view.device ? (
          <span className="flex flex-wrap items-center gap-1.5">
            <span>
              Enter <span className="font-mono font-semibold tracking-wider">{view.device.user_code}</span> on GitHub to
              sign in.
            </span>
            <Button variant="link" onClick={() => githubOpen("device").catch((e) => onError(String(e)))}>
              Open GitHub
            </Button>
            <Button variant="quiet" disabled={busy} onClick={() => act(githubCancelSignIn())}>
              Cancel
            </Button>
          </span>
        ) : view.device_sign_in ? (
          <span className="flex flex-wrap items-center gap-1.5">
            <Button variant="link" disabled={busy} onClick={() => act(githubSignIn())}>
              Sign in with GitHub
            </Button>
            {cancel}
          </span>
        ) : (
          <span className="flex flex-wrap items-center gap-1.5">
            <input
              type="password"
              className={`${inputClass} w-56 min-w-0`}
              placeholder="Paste a GitHub token"
              value={token}
              onChange={(e) => setToken(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && token && act(githubUseToken(token), () => setToken(""))}
            />
            <Button
              variant="link"
              disabled={busy || !token.trim()}
              onClick={() => act(githubUseToken(token), () => setToken(""))}
            >
              Sign in
            </Button>
            <Button variant="quiet" onClick={() => githubOpen("token").catch((e) => onError(String(e)))}>
              Make a token…
            </Button>
            {cancel}
          </span>
        )
      ) : (
        <>
          <span className="flex flex-wrap items-center gap-1.5">
            <input
              className={`${inputClass} w-56 min-w-0`}
              value={name}
              aria-label="Repo"
              onChange={(e) => setRepo(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && name.trim() && act(githubUseRepo(name), onUsed)}
            />
            <Button variant="link" disabled={busy || !name.trim()} onClick={() => act(githubUseRepo(name), onUsed)}>
              {busy ? "Connecting…" : "Use"}
            </Button>
            <Button variant="quiet" disabled={busy} onClick={() => act(githubSignOut())}>
              Sign out
            </Button>
            {cancel}
          </span>
          <p className="text-[11px] text-dim">
            Signed in as {view.login}. Use the same repo on each machine; a new name makes a private one.
          </p>
        </>
      )}
      {view.error && <p className="text-[11px] text-warn">{view.error}</p>}
    </div>
  );
}
