import { useEffect, useRef, useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { rigReportPreview, saveRigReport, type ReportOptions } from "./api";
import { Button, inputClass, Message, Panel, Tile } from "./ui";

// Rig Report: a shareable summary of this machine. Names, formats and versions
// only; the preview is exactly what gets saved or copied.

const KEY = "pp.rig-report";

type Choices = Omit<ReportOptions, "date">;

function loadChoices(machineName: string): Choices {
  const base: Choices = { title: machineName, plugins: true, versions: true, apps: true, parity: true };
  try {
    return { ...base, ...JSON.parse(localStorage.getItem(KEY) ?? "{}") };
  } catch {
    return base;
  }
}

function today(): string {
  return new Date().toLocaleDateString(undefined, { day: "numeric", month: "long", year: "numeric" });
}

/** Copies text, falling back to a hidden textarea where the clipboard API is unavailable. */
async function copyText(text: string) {
  try {
    await navigator.clipboard.writeText(text);
  } catch {
    const t = document.createElement("textarea");
    t.value = text;
    t.style.position = "fixed";
    t.style.opacity = "0";
    document.body.appendChild(t);
    t.select();
    const ok = document.execCommand("copy");
    t.remove();
    if (!ok) throw new Error("Couldn't copy to the clipboard.");
  }
}

function fileName(title: string): string {
  const safe = title.replace(/[\\/:*?"<>|]+/g, " ").replace(/\s+/g, " ").trim();
  return `${safe || "My rig"} - Rig Report`;
}

export function ReportDialog({
  machineName,
  hasGolden,
  onClose,
}: {
  machineName: string;
  hasGolden: boolean;
  onClose: () => void;
}) {
  const [choices, setChoices] = useState(() => loadChoices(machineName));
  const [report, setReport] = useState<{ html: string; markdown: string } | null>(null);
  const [status, setStatus] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const card = useRef<HTMLDivElement>(null);
  const options: ReportOptions = { ...choices, date: today() };

  useEffect(() => {
    card.current?.focus();
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  useEffect(() => {
    try {
      localStorage.setItem(KEY, JSON.stringify(choices));
    } catch {
      // Remembering the choices is a convenience only.
    }
    let live = true;
    rigReportPreview({ ...choices, date: today() }).then(
      (r) => live && setReport(r),
      (e) => live && setError(String(e)),
    );
    return () => {
      live = false;
    };
  }, [choices]);

  const set = (patch: Partial<Choices>) => {
    setStatus(null);
    setChoices((c) => ({ ...c, ...patch }));
  };

  async function saveFile() {
    setError(null);
    setStatus(null);
    try {
      const path = await save({
        defaultPath: `${fileName(choices.title)}.html`,
        filters: [
          { name: "Web page", extensions: ["html"] },
          { name: "Markdown", extensions: ["md"] },
        ],
      });
      if (!path) return;
      await saveRigReport(path, options);
      setStatus("Saved. Open it in any browser, or attach it to a post or email.");
    } catch (e) {
      setError(String(e));
    }
  }

  async function copy() {
    if (!report) return;
    setError(null);
    try {
      await copyText(report.markdown);
      setStatus("Copied as text. Paste it into a forum post, Reddit or a chat.");
    } catch (e) {
      setError(String(e));
    }
  }

  const toggles: [keyof Choices, string, boolean][] = [
    ["plugins", "Every plug-in, by vendor", true],
    ["versions", "Version numbers", true],
    ["apps", "Vendor apps and licensing tools", true],
    ["parity", "Parity score", hasGolden],
  ];

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/30 p-4" onClick={onClose}>
      <div
        ref={card}
        role="dialog"
        aria-modal="true"
        aria-labelledby="report-title"
        tabIndex={-1}
        onClick={(e) => e.stopPropagation()}
        className="flex h-[min(600px,100%)] w-[880px] max-w-full overflow-hidden rounded-lg border border-line bg-panel shadow-xl outline-none"
      >
        <div className="flex w-64 shrink-0 flex-col gap-3 border-r border-line p-4">
          <div>
            <header className="flex items-center gap-2.5">
              <Tile name="doc" className="" />
              <h2 id="report-title" className="text-[15px] font-semibold">
                Rig Report
              </h2>
            </header>
            <p className="mt-2 leading-relaxed text-dim">
              A summary of this machine to share. Names and versions only: no file paths, serials or licence details.
            </p>
          </div>
          <label className="flex flex-col gap-1 text-xs text-dim">
            Title
            <input
              value={choices.title}
              onChange={(e) => set({ title: e.target.value })}
              placeholder="My rig"
              maxLength={80}
              className={inputClass}
            />
          </label>
          <fieldset className="flex flex-col gap-1.5">
            <legend className="mb-1.5 text-xs text-dim">Include</legend>
            <Panel divided>
              {toggles.map(([key, label, enabled]) => (
                <label
                  key={key}
                  className={`flex min-h-8 items-center gap-2.5 px-2.5 py-1.5 text-xs leading-snug ${enabled ? "hover:bg-sel/40" : "text-dim"}`}
                  title={enabled ? undefined : "Make a Primary Rig first"}
                >
                  <input
                    type="checkbox"
                    className="accent-accent"
                    disabled={!enabled}
                    checked={enabled && (choices[key] as boolean)}
                    onChange={(e) => set({ [key]: e.target.checked })}
                  />
                  {label}
                </label>
              ))}
            </Panel>
          </fieldset>
          <span className="flex-1" />
          {status && (
            <Message tone="ok" icon="check">
              {status}
            </Message>
          )}
          {error && (
            <Message tone="bad" icon="info">
              {error}
            </Message>
          )}
          <div className="flex flex-col gap-2">
            <Button onClick={saveFile} disabled={!report}>
              Save as web page…
            </Button>
            <Button variant="quiet" onClick={copy} disabled={!report}>
              Copy as text
            </Button>
            <Button variant="quiet" onClick={onClose}>
              Close
            </Button>
          </div>
        </div>
        <div className="min-w-0 flex-1 bg-canvas">
          {report && (
            <iframe
              title="Rig Report preview"
              srcDoc={report.html}
              sandbox=""
              className="h-full w-full border-0"
            />
          )}
        </div>
      </div>
    </div>
  );
}
