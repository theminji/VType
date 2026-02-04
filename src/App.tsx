import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./App.css";

type Status = "idle" | "recording" | "processing" | "error";

type MicState = "unknown" | "granted" | "denied";

const TARGET_SAMPLE_RATE = 16000;

function App() {
  const [status, setStatus] = useState<Status>("idle");
  const [visible, setVisible] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const statusRef = useRef<Status>("idle");
  const micStateRef = useRef<MicState>("unknown");
  const streamRef = useRef<MediaStream | null>(null);
  const audioContextRef = useRef<AudioContext | null>(null);
  const workletNodeRef = useRef<AudioWorkletNode | null>(null);
  const sourceNodeRef = useRef<MediaStreamAudioSourceNode | null>(null);
  const pcmChunksRef = useRef<Float32Array[]>([]);
  const workletUrlRef = useRef<string | null>(null);
  const recordStartRef = useRef<number | null>(null);
  const recordTimerRef = useRef<number | null>(null);
  const warmStartedRef = useRef(false);
  const lastHotkeyAtRef = useRef(0);
  const minStopAtRef = useRef(0);
  const hasAudioRef = useRef(false);

  useEffect(() => {
    statusRef.current = status;
  }, [status]);

  useEffect(() => {
    const unlistenPromise = listen("hotkey-pressed", () => {
      const now = performance.now();
      if (now - lastHotkeyAtRef.current < 250) {
        return;
      }
      lastHotkeyAtRef.current = now;
      setVisible(true);
      if (!warmStartedRef.current) {
        warmStartedRef.current = true;
        void invoke("warm_asr");
      }
      void toggleRecording();
    });

    return () => {
      void unlistenPromise.then((unlisten) => unlisten());
    };
  }, []);

  useEffect(() => {
    return () => {
      streamRef.current?.getTracks().forEach((track) => track.stop());
      if (audioContextRef.current) {
        void audioContextRef.current.close();
      }
      if (workletUrlRef.current) {
        URL.revokeObjectURL(workletUrlRef.current);
      }
      if (recordTimerRef.current) {
        window.clearInterval(recordTimerRef.current);
      }
    };
  }, []);

  useEffect(() => {
    void verifyMicAccess();
  }, []);

  useEffect(() => {
    if (status !== "idle") {
      return;
    }
    if (!visible) {
      return;
    }
    const timer = window.setTimeout(() => setVisible(false), 450);
    return () => window.clearTimeout(timer);
  }, [status, visible]);

  const ensureStream = async () => {
    // Always acquire a fresh stream to avoid stale tracks after stop.
    if (streamRef.current) {
      streamRef.current.getTracks().forEach((track) => track.stop());
      streamRef.current = null;
    }
    const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
    streamRef.current = stream;
    return stream;
  };

  const startRecording = async () => {
    setError(null);
    try {
      const devices = await navigator.mediaDevices.enumerateDevices();
      const inputs = devices.filter((device) => device.kind === "audioinput");
      console.log(
        "Audio inputs:",
        inputs.map((input) => `${input.label || "Unnamed"} (${input.deviceId})`)
      );
    } catch (err) {
      console.warn("Failed to enumerate audio devices:", err);
    }
    const stream = await ensureStream();
    stream.getAudioTracks().forEach((track) => {
      track.enabled = true;
    });
    pcmChunksRef.current = [];
    hasAudioRef.current = false;

    const audioContext = new AudioContext({ sampleRate: TARGET_SAMPLE_RATE });
    audioContextRef.current = audioContext;
    if (audioContext.sampleRate !== TARGET_SAMPLE_RATE) {
      throw new Error(
        `AudioContext sample rate is ${audioContext.sampleRate}Hz. This app requires ${TARGET_SAMPLE_RATE}Hz.`
      );
    }
    if (audioContext.state !== "running") {
      await audioContext.resume();
    }

    if (!workletUrlRef.current) {
      workletUrlRef.current = createWorkletUrl();
    }

    await audioContext.audioWorklet.addModule(workletUrlRef.current);

    const sourceNode = audioContext.createMediaStreamSource(stream);
    const workletNode = new AudioWorkletNode(audioContext, "pcm-capture", {
      numberOfInputs: 1,
      numberOfOutputs: 1,
      channelCount: 1,
    });
    const gainNode = audioContext.createGain();
    gainNode.gain.value = 0;

    workletNode.port.onmessage = (event) => {
      const data = event.data;
      if (data instanceof Float32Array) {
        pcmChunksRef.current.push(data);
        hasAudioRef.current = true;
      } else if (data instanceof ArrayBuffer) {
        const chunk = new Float32Array(data);
        pcmChunksRef.current.push(chunk);
        hasAudioRef.current = true;
      }
    };

    sourceNode.connect(workletNode);
    workletNode.connect(gainNode);
    gainNode.connect(audioContext.destination);

    sourceNodeRef.current = sourceNode;
    workletNodeRef.current = workletNode;

    recordStartRef.current = audioContext.currentTime;
    minStopAtRef.current = performance.now() + 350;
    if (recordTimerRef.current) {
      window.clearInterval(recordTimerRef.current);
    }
    recordTimerRef.current = window.setInterval(() => {}, 150);

    setStatus("recording");
  };

  const stopRecording = () => {
    if (statusRef.current !== "recording") {
      return;
    }
    if (performance.now() < minStopAtRef.current) {
      return;
    }
    if (!hasAudioRef.current) {
      return;
    }
    setStatus("processing");
    if (recordTimerRef.current) {
      window.clearInterval(recordTimerRef.current);
      recordTimerRef.current = null;
    }
    // Small delay to allow the last worklet buffers to flush.
    window.setTimeout(() => {
      void handleStop();
    }, 120);
  };

  const toggleRecording = async () => {
    const current = statusRef.current;
    if (current === "recording") {
      stopRecording();
      return;
    }
    if (current === "processing") {
      return;
    }
    try {
      await startRecording();
    } catch (err) {
      const errorDetail = formatMicError(err);
      void invoke("log_message", { message: `Mic init error: ${errorDetail}` });
      micStateRef.current = "denied";
      setStatus("error");
      setError(`Microphone error: ${errorDetail}`);
    }
  };

  const verifyMicAccess = async () => {
    try {
      if ("permissions" in navigator && "query" in navigator.permissions) {
        const status = await navigator.permissions.query({
          name: "microphone" as PermissionName,
        });
        if (status.state === "granted") {
          micStateRef.current = "granted";
          return;
        }
        if (status.state === "denied") {
          micStateRef.current = "denied";
          return;
        }
      }

      const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
      stream.getTracks().forEach((track) => track.stop());
      micStateRef.current = "granted";
      console.log("Mic check: getUserMedia succeeded");
    } catch (err) {
      const errorDetail = formatMicError(err);
      micStateRef.current = "denied";
      console.error("Mic check error:", errorDetail);
      void invoke("log_message", { message: `Mic verify error: ${errorDetail}` });
    }
  };

  const handleStop = async () => {
    try {
      const audioContext = audioContextRef.current;
      const sampleRate = audioContext?.sampleRate ?? 44100;
      const outputSampleRate = TARGET_SAMPLE_RATE;

      sourceNodeRef.current?.disconnect();
      workletNodeRef.current?.disconnect();
      sourceNodeRef.current = null;
      workletNodeRef.current = null;
      if (streamRef.current) {
        streamRef.current.getTracks().forEach((track) => track.stop());
        streamRef.current = null;
      }

      if (audioContext) {
        await audioContext.close();
        audioContextRef.current = null;
      }

      const samples = concatFloat32(pcmChunksRef.current);
      if (!samples.length) {
        console.warn("No audio captured.");
        setStatus("error");
        setError("No audio captured. Check microphone input.");
        return;
      }
      console.log(
        "Audio rates:",
        `input=${sampleRate}Hz`,
        `output=${outputSampleRate}Hz`,
        `samples=${samples.length}`
      );
      const stats = getAudioStats(samples);
      console.log(
        "Audio stats:",
        `seconds=${(samples.length / sampleRate).toFixed(2)}`,
        `rms=${stats.rms.toFixed(4)}`,
        `peak=${stats.peak.toFixed(4)}`
      );
      if (samples.length / sampleRate < 0.3) {
        console.warn("Audio too short, transcription may be empty.");
      }
      if (stats.rms < 0.002) {
        console.warn("Audio very quiet, transcription may be empty.");
      }
      const wavBytes = await convertPcmToWav(samples, sampleRate, outputSampleRate);
      const wavBase64 = uint8ToBase64(wavBytes);
      const result = await invoke<string>("transcribe_wav", { wavBase64 });
      if (result) {
        await invoke("paste_transcription", { text: result });
        console.log("Transcription success:", result);
      } else {
        console.warn("Transcription returned empty result");
      }
      setStatus("idle");
    } catch (err) {
      await invoke("log_message", { message: String(err) });
      setStatus("error");
      setError("Transcription failed. Check the backend logs.");
    }
  };

  return (
    <main className={`pill-shell ${status} ${visible ? "visible" : ""}`}>
      <div className="pill">
        <span
          className={`wave ${status === "recording" ? "active" : ""} ${
            status === "processing" ? "loading" : ""
          }`}
        >
          <span className="bar" />
          <span className="bar" />
          <span className="bar" />
          <span className="bar" />
        </span>
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

async function convertPcmToWav(
  samples: Float32Array,
  sampleRate: number,
  targetSampleRate: number
) {
  if (sampleRate !== targetSampleRate) {
    throw new Error(
      `Sample rate mismatch: got ${sampleRate}Hz, expected ${targetSampleRate}Hz`
    );
  }
  return encodeWavFromFloat32(samples, sampleRate);
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

function formatMicError(err: unknown) {
  if (err && typeof err === "object") {
    const name = "name" in err ? String((err as { name?: unknown }).name) : "UnknownError";
    const message =
      "message" in err ? String((err as { message?: unknown }).message) : String(err);
    return `${name}: ${message}`;
  }
  return String(err);
}

function getAudioStats(samples: Float32Array) {
  let peak = 0;
  let sumSquares = 0;
  for (let i = 0; i < samples.length; i += 1) {
    const value = samples[i];
    const abs = Math.abs(value);
    if (abs > peak) {
      peak = abs;
    }
    sumSquares += value * value;
  }
  const rms = samples.length ? Math.sqrt(sumSquares / samples.length) : 0;
  return { rms, peak };
}

export default App;
