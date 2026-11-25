import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import PreferencesWindow from "./PreferencesWindow";
import Onboarding from "./Onboarding";
import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";

const label = isTauri() ? getCurrentWindow().label : "main";
const Window = label === "onboarding" ? Onboarding : label === "settings" ? PreferencesWindow : App;

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <Window />
  </React.StrictMode>,
);
