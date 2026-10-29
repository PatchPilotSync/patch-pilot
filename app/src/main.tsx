import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { onUiChanged, settingsView, type SettingsView } from "./api";
import App from "./App";
import { CrashBoundary } from "./Crash";
import { Tray } from "./screens/Tray";
import "./index.css";

// One front end, two windows: the main window and the tray popover.
const isTray = getCurrentWindow().label === "tray";

// Settings can pin light or dark (decision #96; "system" follows the OS) and
// draw everything a little tighter (compact, decision #97).
function applyLook(v: SettingsView) {
  const root = document.documentElement.dataset;
  if (v.theme === "system") delete root.theme;
  else root.theme = v.theme;
  if (v.compact) root.density = "compact";
  else delete root.density;
}
settingsView().then(applyLook, () => {});

// No webview menu (Reload, Inspect, Back...) on right-click: it isn't Patch
// Pilot's, and Reload drops what's on screen. Text fields keep theirs for
// cut, copy and paste; places with menus of their own (a machine in Compare)
// open them first.
document.addEventListener("contextmenu", (e) => {
  const t = e.target as HTMLElement | null;
  if (!t?.closest("input, textarea, [contenteditable='true']")) e.preventDefault();
});
onUiChanged(applyLook);

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <CrashBoundary updates={!isTray}>{isTray ? <Tray /> : <App />}</CrashBoundary>
  </StrictMode>,
);
