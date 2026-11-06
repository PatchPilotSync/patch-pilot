import { useEffect, useState, type ReactNode } from "react";
import { backupView, onBackupChanged, onSyncChanged, type BackupView } from "./api";
import { placeName } from "./Backup";
import { useShell } from "./shell";
import { Button, Notice, Tile, useFolders, type GlyphName, type Tone } from "./ui";

/** The screen the guide is on: a button to that same screen is left out. */
type Where = "plan" | "setup" | "rig" | "settings";

/**
 * How scripts, templates and presets get to a machine (decision #122). A
 * first-time user found the ways spread over This machine, Get Up To Speed
 * and Settings with nothing tying them together, so this one line names the
 * next thing to set up and opens to the ways in order: link the machines,
 * Bring over what's missing, Keep Files in Sync (a cloud folder, GitHub or
 * the home network), Choose what syncs, and what never travels. Each says
 * whether it's done here and has its button.
 *
 * On a checklist it starts open until syncing is on; elsewhere it starts
 * folded. Once toggled, that's remembered. On This machine it shows only
 * while something is left to set up, since Sync & backup says the rest.
 */
export function FilesGuide({ where }: { where: Where }) {
  const shell = useShell();
  const [view, setView] = useState<BackupView | null>(null);
  useEffect(() => {
    const load = () => backupView().then(setView, () => {});
    load();
    const offBackup = onBackupChanged(load);
    const offSync = onSyncChanged(load);
    return () => {
      offBackup.then((f) => f());
      offSync.then((f) => f());
    };
  }, []);
  const syncing = !!view?.settings.keep_in_sync;
  const checklist = where === "plan" || where === "setup";
  const folders = useFolders("files-guide", () => checklist && !syncing);
  if (!shell || !view) return null;

  const { linked, isPrimary } = shell;
  if (where === "rig" && linked && syncing) return null;
  const place = placeName(view);
  const open = folders.isOpen("guide");

  const linkButton = (
    <Button variant="link" className="h-6 shrink-0 px-2" onClick={shell.link}>
      Link a machine…
    </Button>
  );
  const syncButton = where !== "settings" && (
    <Button variant="link" className="h-6 shrink-0 px-2" onClick={shell.setUpSync}>
      Set up sync…
    </Button>
  );

  let tone: Tone = "accent";
  let text: string;
  let next: ReactNode = null;
  if (!linked) {
    text = syncing
      ? `Syncing through ${place}. Link your other machines so they can share it.`
      : "Link your machines so these can move between them.";
    next = linkButton;
  } else if (!syncing) {
    text = isPrimary
      ? "Turn on Keep Files in Sync so new ones reach your other machines."
      : "Bring over copies them once; Keep Files in Sync keeps them matching.";
    next = syncButton;
  } else if (!view.dir) {
    tone = "warn";
    text = `Keep Files in Sync is on, but ${place} isn't there now (signed out, or not connected).`;
  } else {
    tone = "ok";
    text = `Kept in sync with your other machines through ${place}.`;
  }

  const bringText = isPrimary
    ? "This machine is the Primary Rig, so your other machines bring over its templates, scripts, themes and presets from their Get Up To Speed."
    : `${checklist ? "The Bring over step" : "Get Up To Speed's Bring over step"} copies the Primary Rig's templates, scripts, themes and presets this machine lacks, with the files each needs. Anything replaced is saved first.`;

  return (
    <Notice
      tone={tone}
      icon="copies"
      text={
        <span className="text-dim">
          <span className="font-medium text-fg">Scripts, templates and presets</span> · {text}
        </span>
      }
      actions={!open && next}
      open={open}
      onToggle={() => folders.toggle("guide")}
      toggle={["How it works", "Hide"]}
    >
      <ol className="flex flex-col gap-2">
        <Way glyph="link" title="Link your machines" done={linked && "Linked"} action={!linked && linkButton}>
          Files only move between linked machines. Show a code on one and type it on the other.
        </Way>
        <Way
          glyph="copies"
          title="Bring over what's missing"
          action={
            !checklist &&
            shell.upToSpeed && (
              <Button variant="link" className="h-6 shrink-0 px-2" onClick={shell.upToSpeed}>
                Open Up To Speed
              </Button>
            )
          }
        >
          {bringText}
        </Way>
        <Way
          glyph="sync"
          title="Keep Files in Sync, on every machine"
          done={syncing && `On, through ${place}`}
          action={!syncing && syncButton}
        >
          Pick the same cloud folder (Dropbox, Google Drive, OneDrive, iCloud), private GitHub repo or home network
          on each. New, changed and renamed files then travel both ways on their own; History can undo each.
        </Way>
        <Way
          glyph="sliders"
          title="Choose what syncs"
          action={
            where !== "settings" && (
              <Button variant="link" className="h-6 shrink-0 px-2" onClick={shell.settings}>
                Choose…
              </Button>
            )
          }
        >
          {where === "settings" ? "Under What syncs below" : "In Settings"}, untick a kind to keep it this machine's
          own. DAW settings such as shortcuts and menus change only while that DAW is closed.
        </Way>
      </ol>
      <p className="text-[11px] leading-relaxed text-dim">
        <span className="font-medium text-fg">Never copied:</span> plug-ins, Reaper extensions (SWS, ReaPack,
        ReaImGui) and sample libraries need their own installers, so{" "}
        {checklist ? "they're steps in this checklist" : "Get Up To Speed lists them"}. Reaper's folder settings and
        ReaPack repositories are set from {checklist ? "the" : "Get Up To Speed's"} Reaper setup card. reaper.ini and
        audio and MIDI settings stay with each machine.
      </p>
    </Notice>
  );
}

/** One way files travel: its icon (a green tick once done here), what it is, and its button. */
function Way({
  glyph,
  title,
  done = false,
  action,
  children,
}: {
  glyph: GlyphName;
  title: string;
  /** What's done here, shown in green beside the title. */
  done?: string | false;
  action?: ReactNode;
  children: ReactNode;
}) {
  return (
    <li className="flex items-start gap-2.5">
      <Tile name={done ? "check" : glyph} tone={done ? "ok" : "accent"} className="" />
      <div className="min-w-0 flex-1">
        <p className="font-medium">
          {title}
          {done && <span className="ml-2 text-[11px] font-normal text-ok">{done}</span>}
        </p>
        <p className="text-[11px] leading-relaxed text-dim">{children}</p>
      </div>
      {action}
    </li>
  );
}
