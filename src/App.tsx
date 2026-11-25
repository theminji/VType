import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./App.css";

const TARGET_SAMPLE_RATE = 16000;
const LABELS = {
  idle: "",
  ready: `${navigator.platform.startsWith("Mac") ? "⌘ + ⌥" : "Ctrl + Alt"} + R to dictate`,
  starting: "Opening microphone",
  recording: "Listening",
  processing: "Transcribing",
  cleanup: "Cleaning up text",
  done: "Pasted",
  raw: "Pasted raw transcript",
  empty: "No speech heard",
  error: "",
};

type Status = keyof typeof LABELS;

function App() {
  const [status, setStatus] = useState<Status>("idle");
  const [error, setError] = useState("");
  const current = useRef<Status>("idle");
  const wave = useRef<HTMLSpanElement>(null);

  useEffect(() => {
    let disposed = false;
    let warmed = false;
    let lastHotkey = -Infinity;
    let stream: MediaStream | undefined;
    let context: AudioContext | undefined;
    let animation = 0;
    let dismissal = 0;
    let hide = 0;
    let chunks: Float32Array[] = [];

    function update(next: Status) {
      window.clearTimeout(dismissal);
      window.clearTimeout(hide);
      current.current = next;
      setStatus(next);
      if (next === "idle") {
        hide = window.setTimeout(() => {
          void invoke("hide_overlay").catch(console.warn);
        }, 270);
      }
      if (next === "ready" || next === "done" || next === "raw" || next === "empty" || next === "error") {
        const duration = next === "done" ? 1200 : 3500;
        dismissal = window.setTimeout(() => update("idle"), duration);
      }
    }

    async function releaseAudio() {
      window.cancelAnimationFrame(animation);
      stream?.getTracks().forEach((track) => track.stop());
      stream = undefined;
      const activeContext = context;
      context = undefined;
      if (activeContext && activeContext.state !== "closed") {
        await activeContext.close();
      }
    }

    function report(message: string, detail: unknown) {
      setError(message);
      update("error");
      void invoke("log_message", { message: String(detail) }).catch(console.warn);
    }

    async function startRecording() {
      update("starting");
      if (!warmed) {
        warmed = true;
        void invoke("warm_asr").catch((err) => {
          warmed = false;
          void invoke("log_message", { message: String(err) }).catch(console.warn);
        });
      }

      const mic = await navigator.mediaDevices.getUserMedia({ audio: true });
      if (disposed) {
        mic.getTracks().forEach((track) => track.stop());
        return;
      }

      stream = mic;
      chunks = [];
      const audio = new AudioContext({ sampleRate: TARGET_SAMPLE_RATE });
      context = audio;
      if (audio.sampleRate !== TARGET_SAMPLE_RATE) {
        throw new Error(`Unsupported microphone sample rate: ${audio.sampleRate} Hz`);
      }

      const workletUrl = createWorkletUrl();
      try {
        await audio.audioWorklet.addModule(workletUrl);
      } finally {
        URL.revokeObjectURL(workletUrl);
      }
      if (disposed) return;
      await audio.resume();
      if (disposed) return;

      const source = audio.createMediaStreamSource(mic);
      const analyser = audio.createAnalyser();
      const capture = new AudioWorkletNode(audio, "pcm-capture", {
        channelCount: 1,
        channelCountMode: "explicit",
      });
      const mute = audio.createGain();
      mute.gain.value = 0;
      capture.port.onmessage = (event: MessageEvent<Float32Array>) => chunks.push(event.data);

      source.connect(analyser);
      analyser.connect(capture);
      capture.connect(mute);
      mute.connect(audio.destination);

      const signal = new Float32Array(analyser.fftSize);
      let level = 0;
      function drawMeter() {
        analyser.getFloatTimeDomainData(signal);
        let energy = 0;
        for (const sample of signal) energy += sample * sample;
        const target = Math.min(1, Math.sqrt(energy / signal.length) * 8);
        level += (target - level) * (target > level ? 0.38 : 0.16);
        wave.current?.style.setProperty("--level", level.toFixed(3));
        animation = window.requestAnimationFrame(drawMeter);
      }

      update("recording");
      drawMeter();
    }

    async function stopRecording() {
      update("processing");
      await releaseAudio();
      if (disposed) return;

      const samples = concatFloat32(chunks);
      chunks = [];
      if (!samples.length) {
        update("empty");
        return;
      }

      const wavBase64 = uint8ToBase64(encodeWavFromFloat32(samples, TARGET_SAMPLE_RATE));
      let text: string;
      try {
        text = await invoke<string>("transcribe_wav", { wavBase64 });
      } catch (err) {
        report(String(err).includes("Phonon worker could not start")
          ? "Speech model unavailable" : "Transcription failed", err);
        return;
      }
      if (!text) {
        update("empty");
        return;
      }

      update("cleanup");
      let finalStatus: Status = "done";
      try {
        const cleaned = await invoke<string | null>("cleanup_transcription", { text });
        if (cleaned === null) {
          finalStatus = "raw";
        } else {
          text = cleaned;
        }
      } catch (err) {
        finalStatus = "raw";
        void invoke("log_message", { message: `Cleanup failed: ${String(err)}` }).catch(console.warn);
      }

      try {
        await invoke("paste_transcription", { text });
        update(finalStatus);
      } catch (err) {
        report("Auto-paste failed", err);
      }
    }

    const unlisten = listen("hotkey-pressed", () => {
      const now = performance.now();
      if (disposed || now - lastHotkey < 250) return;
      lastHotkey = now;

      switch (current.current) {
        case "idle":
        case "ready":
        case "done":
        case "raw":
        case "empty":
        case "error":
          void startRecording().catch((err) => {
            void releaseAudio().catch(console.warn);
            if (!disposed) report("Microphone unavailable", err);
          });
          break;
        case "recording":
          void stopRecording().catch((err) => report("Recording failed", err));
          break;
      }
    });

    const unlistenShow = listen("show-overlay", () => {
      if (!disposed && (current.current === "idle" || current.current === "ready")) {
        update("ready");
      }
    });

    return () => {
      disposed = true;
      void unlisten.then((stop) => stop()).catch(console.warn);
      void unlistenShow.then((stop) => stop()).catch(console.warn);
      window.clearTimeout(dismissal);
      window.clearTimeout(hide);
      void releaseAudio().catch(console.warn);
    };
  }, []);

  return (
    <main className="overlay" role="status" aria-live="polite" aria-atomic="true">
      <div className="pill" data-status={status}>
        <span className="meter" ref={wave} aria-hidden="true">
          <i /><i /><i /><i /><i />
        </span>
        <span className="label" key={status}>{status === "error" ? error : LABELS[status]}</span>
      </div>
    </main>
  );
}

function encodeWavFromFloat32(samples: Float32Array, sampleRate: number) {
  const bufferLength = samples.length;
  const wavBuffer = new ArrayBuffer(44 + bufferLength * 2);
  const view = new DataView(wavBuffer);

  writeString(view, 0, "RIFF");
  view.setUint32(4, 36 + bufferLength * 2, true);
  writeString(view, 8, "WAVE");
  writeString(view, 12, "fmt ");
  view.setUint32(16, 16, true);
  view.setUint16(20, 1, true);
  view.setUint16(22, 1, true);
  view.setUint32(24, sampleRate, true);
  view.setUint32(28, sampleRate * 2, true);
  view.setUint16(32, 2, true);
  view.setUint16(34, 16, true);
  writeString(view, 36, "data");
  view.setUint32(40, bufferLength * 2, true);

  let offset = 44;
  for (let i = 0; i < bufferLength; i += 1) {
    const sample = Math.max(-1, Math.min(1, samples[i]));
    view.setInt16(offset, sample < 0 ? sample * 0x8000 : sample * 0x7fff, true);
    offset += 2;
  }

  return new Uint8Array(wavBuffer);
}

function writeString(view: DataView, offset: number, value: string) {
  for (let i = 0; i < value.length; i += 1) {
    view.setUint8(offset + i, value.charCodeAt(i));
  }
}

function uint8ToBase64(data: Uint8Array) {
  let binary = "";
  const chunkSize = 0x8000;
  for (let i = 0; i < data.length; i += chunkSize) {
    const slice = data.subarray(i, i + chunkSize);
    binary += String.fromCharCode(...slice);
  }
  return btoa(binary);
}

function concatFloat32(chunks: Float32Array[]) {
  const totalLength = chunks.reduce((sum, chunk) => sum + chunk.length, 0);
  const result = new Float32Array(totalLength);
  let offset = 0;
  for (const chunk of chunks) {
    result.set(chunk, offset);
    offset += chunk.length;
  }
  return result;
}

function createWorkletUrl() {
  const processorCode = `
class PCMCaptureProcessor extends AudioWorkletProcessor {
  process(inputs) {
    const input = inputs[0];
    if (input && input[0] && input[0].length) {
      const chunk = input[0].slice(0);
      this.port.postMessage(chunk, [chunk.buffer]);
    }
    return true;
  }
}
registerProcessor('pcm-capture', PCMCaptureProcessor);
`;
  const blob = new Blob([processorCode], { type: "application/javascript" });
  return URL.createObjectURL(blob);
}

export default App;
