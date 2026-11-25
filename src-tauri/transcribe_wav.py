import json
import os
import struct
import sys
import wave
from contextlib import redirect_stdout
from functools import lru_cache
from io import BytesIO

import numpy as np

SAMPLE_RATE = 16000
MODEL_NAME = "FermionResearch/Phonon-2.5"
CLEANUP_MODEL = "spokenly/cleanup-1"
CLEANUP_CONTEXT = 4096


def load_asr_model():
    import fermion

    return fermion.load_speech(os.environ.get("VTYPE_MODEL_PATH") or MODEL_NAME)


def transcribe(model, wav_bytes: bytes, hotwords=None) -> str:
    audio = decode_wav_bytes(wav_bytes)
    if not audio.size:
        return ""
    text, _, _ = model.transcribe_array(audio, hotwords=hotwords)
    return text.strip()


@lru_cache(maxsize=1)
def load_cleanup_model():
    from transformers import AutoModelForCausalLM, AutoTokenizer

    tokenizer = AutoTokenizer.from_pretrained(CLEANUP_MODEL)
    model = AutoModelForCausalLM.from_pretrained(CLEANUP_MODEL).eval()
    return tokenizer, model


def clean_transcript(text: str) -> str:
    import torch

    tokenizer, model = load_cleanup_model()
    prompt = f"text: {' '.join(text.split())}\nout:"
    inputs = tokenizer(prompt, return_tensors="pt", add_special_tokens=False)
    prompt_length = inputs.input_ids.shape[-1]
    if prompt_length * 2 >= CLEANUP_CONTEXT:
        return ""

    with torch.inference_mode():
        output = model.generate(
            **inputs,
            do_sample=False,
            max_new_tokens=min(prompt_length + 64, CLEANUP_CONTEXT - prompt_length),
            pad_token_id=tokenizer.eos_token_id,
        )

    generated = output[0, prompt_length:]
    if not len(generated) or generated[-1].item() != model.generation_config.eos_token_id:
        return ""
    return tokenizer.decode(generated, skip_special_tokens=True).strip()


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
                elif operation == b"C":
                    result = clean_transcript(payload.decode("utf-8"))
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
