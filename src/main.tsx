import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import SettingsPanel from "./SettingsPanel";
import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";

const settingsWindow = isTauri() && getCurrentWindow().label === "settings";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    {settingsWindow ? <SettingsPanel /> : <App />}
  </React.StrictMode>,
);
