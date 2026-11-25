import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { LIGHT_HEIGHT, LIGHT_WIDTH, startListeningLight } from "./listeningLight";
import { defaultSettings, type Settings } from "./settings";
import "./Onboarding.css";

const steps = [
  { id: "runtime", name: "Local runtime", description: "Prepares the speech engine" },
  { id: "speech", name: "Phonon 2.5", description: "Transcribes your voice" },
  { id: "formatter", name: "Spokenly Cleanup 1", description: "Cleans up dictated text" },
] as const;

type Stage = typeof steps[number]["id"];
type Phase = "intro" | "installing" | "ready" | "error";
const stageNames = {
  runtime: "Preparing the local runtime",
  speech: "Installing Phonon 2.5",
  formatter: "Installing Spokenly Cleanup 1",
};

export default function Onboarding() {
  const [phase, setPhase] = useState<Phase>("intro");
  const [stage, setStage] = useState<Stage>("runtime");
  const [error, setError] = useState("");
  const [finishing, setFinishing] = useState(false);
  const [shortcut, setShortcut] = useState(defaultSettings.hotkey);
  const [command, setCommand] = useState<string | null>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const phaseRef = useRef<Phase>("intro");

  useEffect(() => {
    const unlisten = listen<Stage>("setup-stage", ({ payload }) => setStage(payload));
    void invoke<string | null>("external_shortcut_command").then(setCommand).catch(console.warn);
    void invoke<Settings>("get_settings").then((settings) => setShortcut(settings.hotkey)).catch(console.warn);
    const stop = canvas.current && startListeningLight(
      canvas.current,
      () => phaseRef.current === "installing" ? "processing" : "ready",
      () => undefined,
      () => 1,
    );
    return () => {
      stop?.();
      void unlisten.then((dispose) => dispose()).catch(console.warn);
    };
  }, []);

  function transition(next: Phase) {
    phaseRef.current = next;
    setPhase(next);
  }

  async function install() {
    transition("installing");
    setStage("runtime");
    setError("");
    try {
      await invoke("install_components");
      transition("ready");
    } catch (failure) {
      setError(String(failure));
      transition("error");
    }
  }

  async function finish() {
    setError("");
    setFinishing(true);
    try {
      await invoke("finish_setup");
    } catch (failure) {
      setError(String(failure));
    } finally {
      setFinishing(false);
    }
  }

  const currentStep = steps.findIndex((step) => step.id === stage);
  const heading = phase === "ready" ? "Ready to dictate"
    : phase === "installing" ? "Setting up VType"
    : phase === "error" ? "Setup paused" : "Set up VType";

  return (
    <main className="setup">
      <header className="setup-header">
        <div className="setup-light" aria-hidden="true">
          <canvas ref={canvas} width={LIGHT_WIDTH} height={LIGHT_HEIGHT} />
        </div>
        <h1>{heading}</h1>
        <p>
          {phase === "ready" ? "VType will type directly into the app you’re using."
            : phase === "installing" ? "You can keep using your computer while setup runs."
            : phase === "error" ? "Check your internet connection and available storage, then try again."
            : "Install two models for voice typing on this device."}
        </p>
      </header>

      <div className="setup-content" key={phase === "ready" ? "ready" : "install"}>
        {phase === "ready" ? (
          <div className="setup-ready">
            {command ? (
              <>
                <p>Add this shortcut to your Hyprland Lua config and apply it through your usual config workflow.</p>
                <input aria-label="Hyprland shortcut line" readOnly
                  onFocus={(event) => event.currentTarget.select()}
                  value={`hl.bind("${shortcut.toUpperCase().split("+").join(" + ")}", hl.dsp.exec_cmd(${JSON.stringify(command)}))`} />
                <p>Press the shortcut once to record, then again to type the transcript.</p>
              </>
            ) : (
              <p>Press <kbd>{shortcut.split("+").join(" + ")}</kbd> to record, then press it again to type the transcript.</p>
            )}
            <p>Double-click the pill to open settings.</p>
          </div>
        ) : (
          <>
            <ul className="setup-steps">
              {steps.map((step, index) => {
                const done = phase !== "intro" && index < currentStep;
                const active = phase === "installing" && index === currentStep;
                return (
                  <li key={step.id} aria-current={active ? "step" : undefined} data-state={done ? "done" : active ? "active" : "pending"}>
                    <span className="setup-step-mark" aria-hidden="true">{done ? "✓" : ""}</span>
                    <span>{step.name}</span>
                    <small>{step.description}</small>
                  </li>
                );
              })}
            </ul>
            <div className="setup-progress" role="status" aria-live="polite">
              {phase === "installing" ? (
                <>
                  <span>{stageNames[stage]}…</span>
                  <div className="setup-track" aria-hidden="true"><i /></div>
                </>
              ) : phase === "intro" ? "About 795 MB of model downloads, plus the local runtime." : null}
            </div>
          </>
        )}
      </div>

      {error && (
        <details className="setup-error">
          <summary>{phase === "ready" ? "Couldn’t finish setup" : "Installation details"}</summary>
          <p>{error}</p>
        </details>
      )}

      <footer>
        {phase === "ready" ? (
          <button disabled={finishing} onClick={() => void finish()}>Finish setup</button>
        ) : (
          <button disabled={phase === "installing"} onClick={() => void install()}>
            {phase === "installing" ? "Installing…" : phase === "error" ? "Try again" : "Install models"}
          </button>
        )}
      </footer>
    </main>
  );
}
