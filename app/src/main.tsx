import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { onUiChanged, settingsView, type SettingsView } from "./api";
import App from "./App";
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
onUiChanged(applyLook);

createRoot(document.getElementById("root")!).render(<StrictMode>{isTray ? <Tray /> : <App />}</StrictMode>);
