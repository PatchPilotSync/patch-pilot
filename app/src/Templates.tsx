import type { ReactNode } from "react";
import { SHOW_IN_FOLDER, revealPath, type TemplateReadiness } from "./api";

/** A template's name, what kind it is and the folder it's in, read from its path. */
export interface TemplateInfo {
  /** `Lead Vocal` */
  label: string;
  /** `Track template` */
  kind: string;
  /** `Vocals`, or empty at the top of its kind's folder. */
  folder: string;
}

/** Reaper's resource folders, by the kind of file they hold. */
const REAPER_KINDS: Record<string, string> = {
  tracktemplates: "Track template",
  projecttemplates: "Project template",
  fxchains: "FX chain",
};

/**
 * `TrackTemplates/Vocals/Lead Vocal.RTrackTemplate` -> Lead Vocal, Track template, Vocals;
 * `Presets/Audio Effects/Audio Effect Rack/Glue.adg` -> Glue, Rack, Audio Effects / Audio Effect Rack.
 */
export function templateInfo(path: string): TemplateInfo {
  const parts = path.split("/").filter(Boolean);
  const file = parts.pop() ?? path;
  const dot = file.lastIndexOf(".");
  const ext = dot > 0 ? file.slice(dot + 1).toLowerCase() : "";
  const label = dot > 0 ? file.slice(0, dot) : file;
  const top = parts[0]?.toLowerCase() ?? "";
  let kind = REAPER_KINDS[top];
  if (kind || top === "templates" || top === "presets") parts.shift();
  if (!kind) {
    if (ext === "als") kind = top === "templates" ? "Live template" : "Live Set";
    else if (ext === "adg") kind = "Rack";
    else if (ext === "rpp") kind = "Project";
    else kind = "Template";
  }
  return { label, kind, folder: parts.join(" / ") };
}

/**
 * One template on one line: its name, then what it is and where (dim), and
 * a folder button for Show in Finder/Explorer when it's on this machine.
 * Hovering the line shows the full path.
 */
export function TemplateLine({
  template: t,
  here,
  extra,
  className = "",
  onError,
}: {
  template: TemplateReadiness;
  /** The template is on this machine, so it can be shown in its folder. */
  here: boolean;
  /** Dim text before the link, e.g. "needs 2". */
  extra?: ReactNode;
  className?: string;
  onError: (message: string) => void;
}) {
  const info = templateInfo(t.name);
  const where = [info.kind, info.folder].filter(Boolean).join(" · ");
  return (
    <li className={`group flex h-6 items-center gap-2 rounded-[5px] hover:bg-sel/50 ${className}`} title={t.path ?? t.name}>
      <span className="max-w-[70%] shrink-0 truncate">{info.label}</span>
      <span className="min-w-0 flex-1 truncate text-[11px] text-dim">{where}</span>
      {extra && <span className="shrink-0 whitespace-nowrap text-[11px] text-dim">{extra}</span>}
      {here && t.path && (
        <button
          onClick={() => revealPath(t.path!).catch((e) => onError(String(e)))}
          title={SHOW_IN_FOLDER}
          aria-label={`${SHOW_IN_FOLDER}: ${info.label}`}
          className="-mr-1 flex h-5 w-5 shrink-0 items-center justify-center rounded text-dim hover:bg-sel hover:text-accent"
        >
          <FolderIcon />
        </button>
      )}
    </li>
  );
}

/** A small open folder, for Show in Finder/Explorer. */
function FolderIcon() {
  return (
    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="m6 14 1.45-2.9A2 2 0 0 1 9.24 10H20a2 2 0 0 1 1.94 2.5l-1.55 6a2 2 0 0 1-1.94 1.5H4a2 2 0 0 1-2-2V5c0-1.1.9-2 2-2h3.93a2 2 0 0 1 1.66.9l.82 1.2a2 2 0 0 0 1.66.9H18a2 2 0 0 1 2 2v2" />
    </svg>
  );
}
