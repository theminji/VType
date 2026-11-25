import { useEffect, useState, type FormEvent, type KeyboardEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import { defaultSettings, type Settings } from "./settings";
import "./SettingsPanel.css";

export default function SettingsPanel() {
  const [settings, setSettings] = useState<Settings>(defaultSettings);
  const [hotwords, setHotwords] = useState("");
  const [capturing, setCapturing] = useState(false);
  const [externalCommand, setExternalCommand] = useState<string | null>(null);
  const [ready, setReady] = useState(false);
  const [saving, setSaving] = useState(false);
  const [message, setMessage] = useState("");
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    void Promise.all([
      invoke<Settings>("get_settings"),
      invoke<string | null>("external_shortcut_command"),
    ]).then(([value, external]) => {
      setExternalCommand(external);
      setSettings(value);
      setHotwords(value.hotwords.join("\n"));
      setReady(true);
    }).catch((error) => {
      setFailed(true);
      setMessage(String(error));
    });
  }, []);

  function update(patch: Partial<Settings>) {
    setMessage("");
    setSettings((current) => ({ ...current, ...patch }));
  }

  function capture(event: KeyboardEvent<HTMLButtonElement>) {
    if (!capturing) return;
    if (event.key === "Tab") {
      setCapturing(false);
      return;
    }
    event.preventDefault();
    if (event.key === "Escape") {
      setCapturing(false);
      return;
    }
    const modifiers = [
      event.ctrlKey && "Ctrl", event.altKey && "Alt",
      event.shiftKey && "Shift", event.metaKey && "Super",
    ].filter(Boolean);
    const key = event.code.replace(/^(Key|Digit)/, "");
    if (modifiers.length && /^[A-Z0-9]$/.test(key)) {
      update({ hotkey: [...modifiers, key].join("+") });
      setCapturing(false);
    }
  }

  async function save(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setCapturing(false);
    setSaving(true);
    setMessage("");
    try {
      const value = await invoke<Settings>("save_settings", {
        settings: { ...settings, hotwords: hotwords.split(/\r?\n/) },
      });
      setSettings(value);
      setHotwords(value.hotwords.join("\n"));
      setFailed(false);
      setMessage("Settings saved.");
    } catch (error) {
      setFailed(true);
      setMessage(String(error));
    } finally {
      setSaving(false);
    }
  }

  const terms = new Set(hotwords.split(/\r?\n/).map((word) => word.trim()).filter(Boolean));

  return (
    <div className="preferences">
      <header>
        <h1>Settings</h1>
        <p>Choose how VType listens and prepares your dictation.</p>
      </header>
      <form onSubmit={save}>
        <fieldset disabled={!ready || saving}>
          <section>
            <div className="setting-row">
              <div>
                <h2>Dictation shortcut</h2>
                <p id="shortcut-hint">Use a modifier plus a letter or number.</p>
              </div>
              <button
                className="shortcut"
                type="button"
                onClick={() => setCapturing(true)}
                onKeyDown={capture}
                onBlur={() => setCapturing(false)}
                aria-describedby="shortcut-hint"
              >
                {capturing ? "Press shortcut…" : settings.hotkey.split("+").join(" + ")}
              </button>
            </div>
            {externalCommand && (
              <label className="hyprland-bind">
                <span className="setting-description">Replace the VType bind in your Hyprland Lua config with this line to apply your shortcut.</span>
                <input type="text" readOnly onFocus={(event) => event.currentTarget.select()}
                  value={`hl.bind("${settings.hotkey.toUpperCase().split("+").join(" + ")}", hl.dsp.exec_cmd(${JSON.stringify(externalCommand)}))`} />
              </label>
            )}
          </section>

          <section>
            <label className="setting-row check">
              <span>
                <span className="setting-title">Format dictated text</span>
                <span className="setting-description">Remove filler words and clean up punctuation with Spokenly.</span>
              </span>
              <input type="checkbox" checked={settings.formatter}
                onChange={(event) => update({ formatter: event.target.checked })} />
            </label>
          </section>

          <section>
            <div className="setting-row">
              <label className="setting-title" htmlFor="hotwords">Hotwords</label>
              <span className="count">{terms.size} / 25</span>
            </div>
            <p id="hotwords-hint">Add names, jargon, or phrases Phonon should listen for. One per line.</p>
            <textarea id="hotwords" aria-describedby="hotwords-hint" rows={3}
              value={hotwords} onChange={(event) => {
                setHotwords(event.target.value);
                setMessage("");
              }} />
          </section>

          <section>
            <label className="setting-row">
              <span>
                <span className="setting-title">Typing speed</span>
                <span className="setting-description">Used to estimate time saved in Analytics.</span>
              </span>
              <span className="typing-speed">
                <input type="number" min={10} max={200} required value={settings.typingSpeedWpm || ""}
                  onChange={(event) => update({ typingSpeedWpm: Number(event.target.value) })} />
                <span>WPM</span>
              </span>
            </label>
          </section>

          <section>
            <div className="setting-row">
              <label className="setting-title" htmlFor="response">Animation sensitivity</label>
              <output htmlFor="response">{settings.meterSensitivity}%</output>
            </div>
            <p id="response-hint">Adjust how strongly the pill responds to your voice.</p>
            <input id="response" type="range" min={50} max={200} step={5}
              aria-describedby="response-hint" value={settings.meterSensitivity}
              onChange={(event) => update({ meterSensitivity: Number(event.target.value) })} />
            <label className="setting-row completion">
              <span>Show confirmation after inserting text</span>
              <input type="checkbox" checked={settings.showCompletion}
                onChange={(event) => update({ showCompletion: event.target.checked })} />
            </label>
          </section>
        </fieldset>
        <footer>
          <span className={failed ? "save-error" : ""} role="status">{message}</span>
          <button className="save" disabled={!ready || saving || terms.size > 25}>{saving ? "Saving…" : "Save changes"}</button>
        </footer>
      </form>
    </div>
  );
}
