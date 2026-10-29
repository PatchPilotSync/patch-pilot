import { useCallback, useEffect, useState, type ReactNode } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  onTransfersChanged,
  sendBigFiles,
  setTransferFolder,
  transferView,
  type CloudFolder,
  type TransferView,
} from "./api";
import { Badge, Button, Card, inputClass, Notice } from "./ui";

/**
 * Big-file hand-off (decision #64): Bring over files too big for the link go
 * through a cloud folder each machine picks (Dropbox, OneDrive, Google Drive,
 * iCloud Drive, ... or any folder). The machine that has them sends them; the
 * one that needs them places them on its own and clears them out. Shows
 * nothing unless some machine needs a big file, so the machine that has them
 * (often the Primary one) sees it on its own screen too.
 */
export function BigFiles({
  onError,
  className = "",
  compact = false,
}: {
  onError: (e: string) => void;
  className?: string;
  /** One notice line that opens to the details, for This machine. */
  compact?: boolean;
}) {
  const [view, setView] = useState<TransferView | null>(null);
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const [sent, setSent] = useState<number | null>(null);
  const [picking, setPicking] = useState(false);

  const load = useCallback(() => {
    transferView().then(setView, (e) => onError(String(e)));
  }, [onError]);
  useEffect(() => {
    load();
    const unlisten = onTransfersChanged(load);
    return () => {
      unlisten.then((f) => f());
    };
  }, [load]);

  if (!view || (view.sendable.length === 0 && view.needed === 0 && view.requested === 0)) return null;

  const current = view.clouds.find((c) => c.path === view.settings.folder);
  const via = view.settings.service === "custom" ? "your folder" : (current?.name ?? "your cloud folder");
  const toSend = view.sendable.filter((f) => !f.sent);

  async function choose(service: string | null, folder: string | null) {
    setBusy(true);
    try {
      setView(await setTransferFolder(service, folder));
      setPicking(false);
    } catch (e) {
      onError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function send() {
    setBusy(true);
    try {
      setSent(await sendBigFiles());
      load();
    } catch (e) {
      onError(String(e));
    } finally {
      setBusy(false);
    }
  }

  const summary =
    view.sendable.length > 0
      ? `${count(view.sendable.length)} your other machines need ${view.sendable.length === 1 ? "is" : "are"} too big for the link`
      : `${count(view.needed)} this machine needs ${view.needed === 1 ? "is" : "are"} too big for the link`;
  const ready = view.dir && !picking;
  const sendButton = ready && toSend.length > 0 && (
    <Button
      variant={compact ? "link" : "primary"}
      onClick={send}
      disabled={busy}
      className={`shrink-0 whitespace-nowrap ${compact ? "h-6 px-2" : ""}`}
    >
      {busy ? "Sending…" : `Send ${toSend.length} via ${via}`}
    </Button>
  );
  const changeButton = ready && (
    <Button variant="quiet" onClick={() => setPicking(true)} className="shrink-0">
      Change folder
    </Button>
  );

  const body = (
    <>
      {(!view.dir || picking) && (
        <FolderPicker
          clouds={view.clouds}
          busy={busy}
          missing={view.settings.folder !== null && !view.dir}
          onChoose={choose}
          onCancel={view.dir ? () => setPicking(false) : undefined}
        />
      )}

      {ready && (
        <>
          {view.sendable.length > 0 && (
            <ul className="flex flex-col">
              {view.sendable.map((f) => (
                <li key={`${f.root}/${f.rel}`} className="flex items-center gap-2 py-1 text-xs">
                  <span className="min-w-0 flex-1 truncate" title={f.rel}>
                    {f.rel.slice(f.rel.lastIndexOf("/") + 1)}
                  </span>
                  <span className="shrink-0 text-[11px] text-dim">
                    {formatMb(f.size)} · for {f.machines.join(", ")}
                  </span>
                  {f.sent && <Badge>Sent, waiting for pickup</Badge>}
                </li>
              ))}
            </ul>
          )}
          <p className="text-[11px] text-dim">
            {sent !== null && sent > 0 && <span className="text-ok">Sent {count(sent)}. </span>}
            {view.last.placed > 0 && <span className="text-ok">Placed {count(view.last.placed)} here. </span>}
            {view.last.waiting > 0 && `${count(view.last.waiting)} still downloading to this machine. `}
            {view.last.no_room && <span className="text-warn">{view.last.no_room} </span>}
            {view.requested > 0 &&
              `${count(view.requested)} asked for, waiting for the machine that has ${view.requested === 1 ? "it" : "them"} to send. `}
            {view.needed > 0 && view.sendable.length === 0
              ? `Send ${view.needed === 1 ? "it" : "them"} from the machine that has ${view.needed === 1 ? "it" : "them"}; it lands here by itself.`
              : `Each machine places its files by itself, and they leave ${via} once every machine has them.`}
          </p>
        </>
      )}
    </>
  );

  if (compact) {
    return (
      <Notice
        tone={view.last.no_room ? "warn" : "accent"}
        text={ready ? `${summary}. They travel through ${via}.` : `${summary}. Pick a cloud folder to carry them.`}
        open={open}
        onToggle={() => setOpen((o) => !o)}
        actions={sendButton}
      >
        {changeButton && <div className="-mb-1 -mt-1 flex justify-end">{changeButton}</div>}
        {body}
      </Notice>
    );
  }

  return (
    <Card
      title="Big files"
      sub={`${summary}.`}
      className={className}
      actions={
        <>
          {changeButton}
          {sendButton}
        </>
      }
    >
      <div className="mt-2 flex flex-col gap-1.5">{body}</div>
    </Card>
  );
}

/** Pick a cloud folder from those found here, or any folder. Shared with Backup. */
export function FolderPicker({
  clouds,
  busy,
  missing,
  onChoose,
  onCancel,
  preferred,
  hint,
}: {
  clouds: CloudFolder[];
  busy: boolean;
  missing: boolean;
  onChoose: (service: string | null, folder: string | null) => void;
  onCancel?: () => void;
  /** The folder to offer first, if it's in the list. */
  preferred?: string | null;
  /** The line above the list. */
  hint?: ReactNode;
}) {
  const [pick, setPick] = useState(
    clouds.find((c) => c.path === preferred)?.path ?? clouds[0]?.path ?? "",
  );

  async function chooseFolder() {
    const folder = await open({ directory: true }).catch(() => null);
    if (typeof folder === "string") onChoose("custom", folder);
  }

  return (
    <div className="flex flex-col gap-1.5 text-xs">
      {missing && <p className="text-warn">The folder picked before isn't there now (signed out, or not connected).</p>}
      <p className="text-dim">
        {hint ?? (
          <>
            Pick a cloud folder to carry them, the same service on each machine. They go in a{" "}
            <span className="font-medium">Patch Pilot Transfers</span> folder there.
          </>
        )}
      </p>
      <span className="flex flex-wrap items-center gap-1.5">
        {clouds.length > 0 && (
          <>
            <select
              className={`${inputClass} min-w-0`}
              value={pick}
              onChange={(e) => setPick(e.target.value)}
            >
              {clouds.map((c) => (
                <option key={c.path} value={c.path} title={c.path}>
                  {c.name}
                </option>
              ))}
            </select>
            {/* A link, so the step or panel around it keeps its one filled button. */}
            <Button
              variant="link"
              disabled={busy || !pick}
              onClick={() => {
                const c = clouds.find((x) => x.path === pick);
                if (c) onChoose(c.id, c.path);
              }}
            >
              Use
            </Button>
          </>
        )}
        <Button variant="quiet" disabled={busy} onClick={chooseFolder}>
          {clouds.length > 0 ? "Another folder…" : "Choose a folder…"}
        </Button>
        {onCancel && (
          <Button variant="quiet" onClick={onCancel}>
            Cancel
          </Button>
        )}
      </span>
      {clouds.length === 0 && (
        <p className="text-dim">
          No Dropbox, OneDrive, Google Drive, iCloud Drive, Box, pCloud, Sync.com or MEGA folder here. Any folder that
          syncs between your machines works, a NAS share too.
        </p>
      )}
    </div>
  );
}

function count(n: number): string {
  return `${n} file${n === 1 ? "" : "s"}`;
}

function formatMb(bytes: number): string {
  return `${(bytes / 1_000_000).toFixed(1)} MB`;
}
