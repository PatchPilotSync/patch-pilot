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
import { Badge, Button } from "./ui";

/**
 * Big-file hand-off (decision #64): Bring over files too big for the link go
 * through a cloud folder each machine picks (Dropbox, OneDrive, Google Drive,
 * iCloud Drive, ... or any folder). The machine that has them sends them; the
 * one that needs them places them on its own and clears them out. Shows
 * nothing unless some machine needs a big file, so the machine that has them
 * (often the Primary one) sees it on its own screen too.
 */
export function BigFiles({ onError, className = "" }: { onError: (e: string) => void; className?: string }) {
  const [view, setView] = useState<TransferView | null>(null);
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

  if (!view || (view.sendable.length === 0 && view.needed === 0)) return null;

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

  return (
    <section className={`rounded-lg border border-line bg-panel px-3 py-2.5 ${className}`}>
      <header className="flex items-center gap-3">
        <div className="min-w-0 flex-1">
          <h2 className="text-xs font-semibold">Big files</h2>
          <p className="text-[11px] text-dim">
            {view.sendable.length > 0
              ? `${count(view.sendable.length)} your other machines need ${view.sendable.length === 1 ? "is" : "are"} too big for the link.`
              : `${count(view.needed)} this machine needs ${view.needed === 1 ? "is" : "are"} too big for the link.`}
          </p>
        </div>
        {view.dir && !picking && (
          <Button variant="quiet" onClick={() => setPicking(true)} className="shrink-0">
            Change folder
          </Button>
        )}
        {view.dir && !picking && toSend.length > 0 && (
          <Button onClick={send} disabled={busy} className="shrink-0 whitespace-nowrap">
            {busy ? "Sending…" : `Send ${toSend.length} via ${via}`}
          </Button>
        )}
      </header>

      {(!view.dir || picking) && (
        <FolderPicker
          clouds={view.clouds}
          busy={busy}
          missing={view.settings.folder !== null && !view.dir}
          onChoose={choose}
          onCancel={view.dir ? () => setPicking(false) : undefined}
        />
      )}

      {view.dir && !picking && (
        <>
          {view.sendable.length > 0 && (
            <ul className="mt-2 flex flex-col">
              {view.sendable.map((f) => (
                <li key={`${f.root}/${f.rel}`} className="flex items-center gap-2 px-1 py-1 text-xs">
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
          <p className="mt-1.5 text-[11px] text-dim">
            {sent !== null && sent > 0 && <span className="text-ok">Sent {count(sent)}. </span>}
            {view.last.placed > 0 && <span className="text-ok">Placed {count(view.last.placed)} here. </span>}
            {view.last.waiting > 0 && `${count(view.last.waiting)} still downloading to this machine. `}
            {view.needed > 0 &&
              view.sendable.length === 0 &&
              "Send them from the machine that has them, through the same service. "}
            The machine that needs a file puts it in place by itself (you can undo it in History), and it's deleted from{" "}
            {via} once every machine has it.
          </p>
        </>
      )}
    </section>
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
    <div className="mt-2 flex flex-col gap-1.5 text-xs">
      {missing && <p className="text-warn">The folder picked before isn't there now (signed out, or not connected).</p>}
      <p className="text-dim">
        {hint ?? (
          <>
            Pick a cloud folder to carry them. Use the same service on each machine; Patch Pilot keeps them in a{" "}
            <span className="font-medium">Patch Pilot Transfers</span> folder there.
          </>
        )}
      </p>
      <span className="flex flex-wrap items-center gap-1.5">
        {clouds.length > 0 && (
          <>
            <select
              className="h-7 min-w-0 rounded-[5px] border border-line bg-panel px-1.5"
              value={pick}
              onChange={(e) => setPick(e.target.value)}
            >
              {clouds.map((c) => (
                <option key={c.path} value={c.path} title={c.path}>
                  {c.name}
                </option>
              ))}
            </select>
            <Button
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
          No Dropbox, OneDrive, Google Drive, iCloud Drive, Box, pCloud, Sync.com or MEGA folder found here. Any folder
          that syncs between your machines works, including a NAS share.
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
