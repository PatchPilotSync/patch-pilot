import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { getCurrentWindow } from "@tauri-apps/api/window";
import App from "./App";
import { Tray } from "./screens/Tray";
import "./index.css";

// One front end, two windows: the main window and the tray popover.
const isTray = getCurrentWindow().label === "tray";

createRoot(document.getElementById("root")!).render(<StrictMode>{isTray ? <Tray /> : <App />}</StrictMode>);
