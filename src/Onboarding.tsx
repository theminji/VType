import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "@fontsource/instrument-serif/latin-400.css";
import "@fontsource-variable/instrument-sans";
import { startSetupOrb, type SamplePhase, type SetupPhase, type StudyState } from "./setupOrb";
import { defaultSettings, type Settings } from "./settings";
import "./Onboarding.css";

const steps = [
  { id: "runtime", name: "Prepare the local runtime" },
  { id: "speech", name: "Install Phonon 2.5" },
  { id: "formatter", name: "Install Spokenly Cleanup" },
] as const;

const sampleText = "The best ideas rarely wait for a keyboard.";
type Stage = typeof steps[number]["id"];

function nextSample(sample: SamplePhase): SamplePhase {
  return sample === "listening" ? "text" : "listening";
}

const copy = {
  intro: { heading: ["Speak it.", "See it."], body: "Dictate into the app you already have open. Your voice stays on this device." },
  installing: { heading: ["Getting", "ready."], body: "Your models are being set up. You can try the sample while you wait." },
  error: { heading: ["Setup", "paused."], body: "Check your connection and available storage, then try again." },
  ready: { heading: ["Ready to", "dictate."], body: "Open a text field and press your shortcut to record. Press it again to insert the transcript." },
};

export default function Onboarding() {
  const [phase, setPhase] = useState<SetupPhase>("intro");
  const [stage, setStage] = useState<Stage>("runtime");
  const [sample, setSample] = useState<SamplePhase>("idle");
  const [error, setError] = useState("");
  const [finishing, setFinishing] = useState(false);
  const [shortcut, setShortcut] = useState(defaultSettings.hotkey);
  const [command, setCommand] = useState<string | null>(null);
  const [copyStatus, setCopyStatus] = useState("");
  const canvas = useRef<HTMLCanvasElement>(null);
  const artwork = useRef<HTMLImageElement>(null);
  const bindField = useRef<HTMLInputElement>(null);
  const orb = useRef<ReturnType<typeof startSetupOrb>>(undefined);
  const flow = useRef<StudyState>({ phase: "intro", stage: 0, sample: "idle" });
  const currentStep = steps.findIndex((step) => step.id === stage);

  useEffect(() => {
    const unlisten = listen<Stage>("setup-stage", ({ payload }) => setStage(payload));
    const unlistenShortcut = listen("onboarding-shortcut", () => setSample(nextSample));
    void invoke<string | null>("external_shortcut_command").then(setCommand).catch(console.warn);
    void invoke<Settings>("get_settings").then((settings) => setShortcut(settings.hotkey)).catch(console.warn);
    if (canvas.current && artwork.current) {
      orb.current = startSetupOrb(canvas.current, artwork.current, () => flow.current);
    }
    return () => {
      orb.current?.stop();
      void unlisten.then((dispose) => dispose()).catch(console.warn);
      void unlistenShortcut.then((dispose) => dispose()).catch(console.warn);
    };
  }, []);

  useEffect(() => {
    flow.current = { phase, stage: currentStep, sample };
    orb.current?.update();
  }, [phase, currentStep, sample]);

  async function install() {
    setPhase("installing");
    setStage("runtime");
    setError("");
    try {
      await invoke("install_components");
      setPhase("ready");
    } catch (failure) {
      setError(String(failure));
      setPhase("error");
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

  const bind = command
    ? `hl.bind("${shortcut.toUpperCase().split("+").join(" + ")}", hl.dsp.exec_cmd(${JSON.stringify(command)}))`
    : null;

  async function copyBind() {
    if (!bind) return;
    try {
      await navigator.clipboard.writeText(bind);
      setCopyStatus("Shortcut copied.");
    } catch {
      bindField.current?.focus();
      bindField.current?.select();
      setCopyStatus("Shortcut selected. Press Ctrl+C to copy.");
    }
  }

  return (
    <main className="setup" data-phase={phase}>
      <div className="setup-panel">
        <div className="setup-brand">VType</div>
        <div className="setup-main">
          <div key={phase} className="setup-chapter">
            <h1><span>{copy[phase].heading[0]}</span>{" "}<span>{copy[phase].heading[1]}</span></h1>
            <p className="setup-lead">{copy[phase].body}</p>
          </div>

          <div className="setup-status" role="status">
            {phase === "installing" && <span className="sr-only">{steps[currentStep]?.name} in progress.</span>}
            {phase === "ready" && <span className="sr-only">Installation complete. Ready to dictate.</span>}
            {phase === "error" && <span className="sr-only">Setup paused. You can try again.</span>}
          </div>
          {phase === "installing" && (
            <ol className="setup-stages" aria-label="Installation stages">
              {steps.map((step, index) => (
                <li key={step.id} aria-current={currentStep === index ? "step" : undefined}
                  data-state={index < currentStep ? "done" : index === currentStep ? "active" : "pending"}>
                  {step.name}
                  {index < currentStep && <span className="sr-only"> complete</span>}
                </li>
              ))}
            </ol>
          )}
          {phase === "ready" && (
            <section className="setup-shortcut" aria-label="Dictation shortcut">
              <div className="setup-keys" aria-label={shortcut.split("+").join(" + ")}>
                {shortcut.split("+").map((key) => <kbd key={key}>{key}</kbd>)}
              </div>
              {bind && (
                <details className="setup-bind" open>
                  <summary>Set up the Hyprland shortcut</summary>
                  <p>Add this line to your Hyprland Lua config, then apply your config.</p>
                  <div className="setup-bind-row">
                    <input ref={bindField} readOnly aria-label="Hyprland shortcut line"
                      value={bind} onFocus={(event) => event.currentTarget.select()} />
                    <button type="button" onClick={() => void copyBind()}>Copy</button>
                  </div>
                  <p role="status">{copyStatus}</p>
                </details>
              )}
              <p className="setup-tip">Double-click the dictation pill to open settings.</p>
            </section>
          )}
          {error && (
            <details className="setup-error">
              <summary>{phase === "ready" ? "Couldn’t finish setup" : "Installation details"}</summary>
              <p>{error}</p>
            </details>
          )}
        </div>

        <footer className="setup-actions">
          <button className="setup-primary" type="button"
            disabled={phase === "installing" || finishing}
            onClick={() => void (phase === "ready" ? finish() : install())}>
            {phase === "ready" ? "Finish setup" : phase === "installing" ? "Installing…"
              : phase === "error" ? "Try again" : "Install models"}
          </button>
          {phase === "intro" && <p>795 MB of models, plus the local runtime.</p>}
        </footer>
      </div>

      <section className="setup-study" data-sample={sample} aria-label="Dictation preview">
        <img ref={artwork} src="/setup-liquid.webp" alt="" />
        <canvas ref={canvas} aria-hidden="true" />
        <div className="setup-sample">
          <div className="setup-sample-result" role="status" aria-live="polite">
            {sample === "text" ? (
              <p aria-label={sampleText}>
                {sampleText.split(" ").map((word, index) => (
                  <span key={index} aria-hidden="true" style={{ animationDelay: `${index * 48}ms` }}>{word}{" "}</span>
                ))}
              </p>
            ) : sample === "listening" ? <p>Listening to the sample…</p> : null}
          </div>
          <button type="button" className="setup-sample-button"
            onClick={() => setSample(nextSample)}>
            <span className="setup-sample-mark" aria-hidden="true" />
            {sample === "listening" ? "Insert sample text" : sample === "text" ? "Replay sample" : "Try a sample"}
          </button>
        </div>
      </section>
    </main>
  );
}
