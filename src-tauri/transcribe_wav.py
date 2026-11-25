import json
import os
import struct
import sys
import wave
from contextlib import redirect_stdout
from io import BytesIO

import numpy as np

SAMPLE_RATE = 16000
MODEL_NAME = "FermionResearch/Phonon-2.5"
CLEANUP_MODEL = "spokenly/cleanup-1-gguf"
CLEANUP_FILE = "cleanup-1-q8_0.gguf"
CLEANUP_REVISION = "79936c95e5c0ea9162ae7a435e64cb3127020e36"


def load_asr_model():
    import fermion

    return fermion.load_speech(os.environ.get("VTYPE_MODEL_PATH") or MODEL_NAME)


def transcribe(model, wav_bytes: bytes, hotwords=None) -> str:
    audio = decode_wav_bytes(wav_bytes)
    if not audio.size:
        return ""
    text, _, _ = model.transcribe_array(audio, hotwords=hotwords)
    return text.strip()


def download_cleanup_model() -> str:
    from huggingface_hub import hf_hub_download

    return hf_hub_download(CLEANUP_MODEL, CLEANUP_FILE, revision=CLEANUP_REVISION)


def decode_wav_bytes(wav_bytes: bytes) -> np.ndarray:
    with wave.open(BytesIO(wav_bytes), "rb") as wf:
        if wf.getnchannels() != 1 or wf.getsampwidth() != 2:
            raise ValueError("Input WAV must be mono 16-bit PCM")
        if wf.getframerate() != SAMPLE_RATE:
            raise ValueError(f"Input WAV must be {SAMPLE_RATE}Hz")
        pcm = wf.readframes(wf.getnframes())
    return np.frombuffer(pcm, dtype="<i2").astype(np.float32) / 32768.0


def run_worker() -> int:
    out = sys.stdout.buffer
    with redirect_stdout(sys.stderr):
        try:
            model = load_asr_model()
        except (Exception, SystemExit) as exc:
            detail = " ".join(str(exc).splitlines())
            out.write(f"ERROR: {detail}\n".encode("utf-8"))
            out.flush()
            return 1

    out.write(b"ready\n")
    out.flush()
    buf = sys.stdin.buffer

    while operation := buf.read(1):
        header = buf.read(4)
        if len(header) != 4:
            break
        length = struct.unpack("<I", header)[0]
        payload = buf.read(length)
        if len(payload) != length:
            break

        try:
            with redirect_stdout(sys.stderr):
                if operation == b"T":
                    (terms_size,) = struct.unpack_from("<I", payload)
                    wav_offset = 4 + terms_size
                    hotwords = json.loads(payload[4:wav_offset])
                    result = transcribe(model, payload[wav_offset:], hotwords)
                elif operation == b"F":
                    result = download_cleanup_model()
                else:
                    raise ValueError("Unknown speech worker operation")
            status = b"\x00"
        except Exception as exc:
            status = b"\x01"
            result = str(exc) or type(exc).__name__

        text = result.encode("utf-8")
        out.write(status + struct.pack("<I", len(text)))
        out.write(text)
        out.flush()
    return 0


def main() -> int:
    if "--worker" in sys.argv:
        return run_worker()
    wav_bytes = sys.stdin.buffer.read()
    if not wav_bytes:
        print("Missing input wav bytes on stdin", file=sys.stderr)
        return 1
    with redirect_stdout(sys.stderr):
        text = transcribe(load_asr_model(), wav_bytes)
    print(text)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
