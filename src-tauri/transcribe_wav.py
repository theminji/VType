import struct
import sys
import wave
from io import BytesIO
from pathlib import Path
import os
import tempfile
import shutil

import numpy as np

SAMPLE_RATE = 16000
MODEL_NAME = "nemo-parakeet-tdt-0.6b-v3"


def app_base_path() -> Path:
    local_appdata = os.getenv("LOCALAPPDATA")
    if local_appdata:
        return Path(local_appdata) / "vtype"
    return Path(tempfile.gettempdir()) / "vtype"


def configure_hf_cache() -> None:
    base = app_base_path()
    hf_home = base / "hf"
    hf_hub_cache = hf_home / "hub"
    xdg_cache = base / "xdg-cache"

    hf_home.mkdir(parents=True, exist_ok=True)
    hf_hub_cache.mkdir(parents=True, exist_ok=True)
    xdg_cache.mkdir(parents=True, exist_ok=True)

    # Keep caches in a trusted local app directory on Windows.
    os.environ.setdefault("HF_HOME", str(hf_home))
    os.environ.setdefault("HUGGINGFACE_HUB_CACHE", str(hf_hub_cache))
    os.environ.setdefault("XDG_CACHE_HOME", str(xdg_cache))


configure_hf_cache()
import onnx_asr


def model_path() -> Path:
    override = os.getenv("VTYPE_MODEL_PATH")
    if override:
        return Path(override)

    base = app_base_path() / "models"
    return base / MODEL_NAME


def load_asr_model():
    path = model_path()
    path.parent.mkdir(parents=True, exist_ok=True)
    try:
        return onnx_asr.load_model(MODEL_NAME, path=path)
    except Exception:
        # If a partial/corrupt model directory exists, clear and retry once.
        if path.exists():
            shutil.rmtree(path, ignore_errors=True)
            return onnx_asr.load_model(MODEL_NAME, path=path)
        raise


def decode_wav_bytes(wav_bytes: bytes) -> np.ndarray:
    with wave.open(BytesIO(wav_bytes), "rb") as wf:
        if wf.getnchannels() != 1 or wf.getsampwidth() != 2:
            raise ValueError("Input WAV must be mono 16-bit PCM")
        if wf.getframerate() != SAMPLE_RATE:
            raise ValueError(f"Input WAV must be {SAMPLE_RATE}Hz")
        pcm = wf.readframes(wf.getnframes())
    return np.frombuffer(pcm, dtype=np.int16).astype(np.float32) / 32768.0


def run_worker() -> int:
    model = load_asr_model()
    sys.stdout.write("ready\n")
    sys.stdout.flush()

    buf = sys.stdin.buffer
    out = sys.stdout.buffer

    while True:
        header = buf.read(4)
        if not header:
            break
        length = struct.unpack("<I", header)[0]
        if length == 0:
            out.write(struct.pack("<I", 0))
            out.flush()
            continue
        payload = buf.read(length)
        if len(payload) < length:
            break
        try:
            audio = decode_wav_bytes(payload)
            result = model.recognize(audio, sample_rate=SAMPLE_RATE)
            if isinstance(result, list):
                result = " ".join(str(item) for item in result)
            text = str(result).strip().encode("utf-8")
            out.write(struct.pack("<I", len(text)))
            out.write(text)
            out.flush()
        except Exception as exc:
            err = f"ERROR: {exc}".encode("utf-8")
            out.write(struct.pack("<I", len(err)))
            out.write(err)
            out.flush()
    return 0


def main() -> int:
    if "--worker" in sys.argv:
        return run_worker()
    wav_bytes = sys.stdin.buffer.read()
    if not wav_bytes:
        print("Missing input wav bytes on stdin", file=sys.stderr)
        return 1
    audio = decode_wav_bytes(wav_bytes)
    model = load_asr_model()
    text = model.recognize(audio, sample_rate=SAMPLE_RATE)
    print(text)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
