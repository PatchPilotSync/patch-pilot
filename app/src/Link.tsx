import { useEffect, useRef, useState } from "react";
import { linkJoin, linkStart, onLinkFinished, unlink, type SyncStatus } from "./api";
import { Button, Message, NavTab, Notice, Panel, Tile } from "./ui";

// Linking machines for live sync (decision #38). One machine shows a short
// code; the other types it. Everything after that is automatic.

type Mode = { kind: "choose" } | { kind: "show"; code: string } | { kind: "enter" } | { kind: "done"; text: string };

/** A dot and a word for the top bar: linked, offline or not linked yet. Says "linked", not
 * "sync", so it isn't confused with keeping files in sync (decision #70). */
export function SyncBadge({ status, onClick }: { status: SyncStatus | null; onClick: () => void }) {
  const [dot, label] = !status?.linked
    ? ["bg-dim", "Link a machine"]
    : status.online
      ? ["bg-ok", "Machines linked"]
      : ["bg-warn", "Link offline"];
  return (
    <NavTab onClick={onClick} title={status?.linked && !status.online && status.error ? status.error : undefined}>
      <span className={`h-1.5 w-1.5 shrink-0 rounded-full ${dot}`} />
      {label}
    </NavTab>
  );
}

export function LinkDialog({ status, onClose }: { status: SyncStatus | null; onClose: () => void }) {
  const [mode, setMode] = useState<Mode>({ kind: "choose" });
  const [code, setCode] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const card = useRef<HTMLDivElement>(null);

  useEffect(() => {
    card.current?.focus();
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    const off = onLinkFinished((r) =>
      r.ok
        ? setMode({ kind: "done", text: "Linked. The other machine will show up in Compare in a moment." })
        : setError(r.error ?? "Linking didn't finish."),
    );
    return () => {
      window.removeEventListener("keydown", onKey);
      off.then((f) => f());
    };
  }, [onClose]);

  async function show() {
    setError(null);
    setBusy(true);
    try {
      setMode({ kind: "show", code: await linkStart() });
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function join() {
    setError(null);
    setBusy(true);
    try {
      await linkJoin(code);
      setMode({ kind: "done", text: "Linked. Your other machines will show up in Compare in a moment." });
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function stop() {
    setError(null);
    try {
      await unlink();
      setMode({ kind: "done", text: "This machine is unlinked. Machines it already received stay in Compare." });
    } catch (e) {
      setError(String(e));
    }
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/30 p-4" onClick={onClose}>
      <div
        ref={card}
        role="dialog"
        aria-modal="true"
        aria-labelledby="link-title"
        tabIndex={-1}
        onClick={(e) => e.stopPropagation()}
        className="flex w-[380px] max-w-full flex-col gap-3 rounded-lg border border-line bg-panel p-5 shadow-xl outline-none"
      >
        <header className="flex items-center gap-2.5">
          <Tile name="link" className="" />
          <h2 id="link-title" className="text-[15px] font-semibold">
            Link your machines
          </h2>
        </header>

        {mode.kind === "choose" && (
          <>
            <p className="leading-relaxed text-dim">
              Linked machines see each other in Compare and update live. Only the lists of what's installed are shared,
              end-to-end encrypted. Plug-ins and files never leave the machine.
            </p>
            {status?.linked && (
              <Panel className="p-1">
                <Notice
                  tone={status.online ? "ok" : "warn"}
                  icon={status.online ? "check" : "info"}
                  text={
                    <span title={status.error ?? undefined}>
                      This machine is linked
                      {status.online ? " and online" : status.error ? `, offline: ${status.error}` : ", offline"}.
                    </span>
                  }
                />
              </Panel>
            )}
            <div className="flex flex-col gap-2">
              <Button disabled={busy} onClick={show}>
                Show a code for another machine
              </Button>
              {!status?.linked && (
                <Button variant="quiet" onClick={() => setMode({ kind: "enter" })}>
                  Enter a code from another machine
                </Button>
              )}
              {status?.linked && (
                <Button variant="quiet" onClick={stop}>
                  Unlink this machine
                </Button>
              )}
            </div>
          </>
        )}

        {mode.kind === "show" && (
          <>
            <p className="leading-relaxed text-dim">
              On the other machine, open Patch Pilot, click Link a machine at the top, choose Enter a code, and type:
            </p>
            <p className="select-all rounded-lg border border-line bg-canvas py-3 text-center font-mono text-2xl font-semibold tracking-wider">
              {mode.code}
            </p>
            <p className="text-center text-xs text-dim">Waiting for the other machine. The code works once, for 10 minutes.</p>
          </>
        )}

        {mode.kind === "enter" && (
          <form
            className="flex flex-col gap-2"
            onSubmit={(e) => {
              e.preventDefault();
              join();
            }}
          >
            <p className="leading-relaxed text-dim">
              On your linked machine, click Link a machine at the top and choose Show a code. Then type it here.
            </p>
            <input
              autoFocus
              value={code}
              onChange={(e) => setCode(e.target.value)}
              placeholder="7K2M-QX4P-9TRA"
              spellCheck={false}
              autoCapitalize="characters"
              className="h-10 rounded-md border border-line bg-canvas px-2 text-center font-mono text-lg tracking-wider text-fg outline-none placeholder:text-dim focus:border-accent"
            />
            <Button type="submit" disabled={busy || code.replace(/[\s-]/g, "").length !== 12}>
              {busy ? "Linking…" : "Link"}
            </Button>
          </form>
        )}

        {mode.kind === "done" && (
          <Message tone="ok" icon="check">
            {mode.text}
          </Message>
        )}

        {error && (
          <Message tone="bad" icon="info">
            {error}
          </Message>
        )}

        <div className="flex justify-end">
          <Button variant="quiet" onClick={onClose}>
            {mode.kind === "done" ? "Done" : "Close"}
          </Button>
        </div>
      </div>
    </div>
  );
}
