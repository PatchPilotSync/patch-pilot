import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { onUiChanged, settingsView, type Theme } from "./api";
import App from "./App";
import { Tray } from "./screens/Tray";
import "./index.css";

// One front end, two windows: the main window and the tray popover.
const isTray = getCurrentWindow().label === "tray";

// Settings can pin light or dark (decision #96); "system" follows the OS.
function applyTheme(theme: Theme) {
  if (theme === "system") delete document.documentElement.dataset.theme;
  else document.documentElement.dataset.theme = theme;
}
settingsView().then((v) => applyTheme(v.theme), () => {});
onUiChanged((v) => applyTheme(v.theme));

createRoot(document.getElementById("root")!).render(<StrictMode>{isTray ? <Tray /> : <App />}</StrictMode>);
