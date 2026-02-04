import struct
import sys
import wave
from io import BytesIO

import numpy as np
import onnx_asr

SAMPLE_RATE = 16000


def decode_wav_bytes(wav_bytes: bytes) -> np.ndarray:
    with wave.open(BytesIO(wav_bytes), "rb") as wf:
        if wf.getnchannels() != 1 or wf.getsampwidth() != 2:
            raise ValueError("Input WAV must be mono 16-bit PCM")
        if wf.getframerate() != SAMPLE_RATE:
            raise ValueError(f"Input WAV must be {SAMPLE_RATE}Hz")
        pcm = wf.readframes(wf.getnframes())
    return np.frombuffer(pcm, dtype=np.int16).astype(np.float32) / 32768.0


def run_worker() -> int:
    model = onnx_asr.load_model("nemo-parakeet-tdt-0.6b-v3")
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
    model = onnx_asr.load_model("nemo-parakeet-tdt-0.6b-v3")
    text = model.recognize(audio, sample_rate=SAMPLE_RATE)
    print(text)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
