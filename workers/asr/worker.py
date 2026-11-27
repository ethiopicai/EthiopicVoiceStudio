"""Private one-job offline worker. Stdout is versioned progress JSON, never user text."""
import argparse
import contextlib
import json
import math
import os
from pathlib import Path
import sys
import wave


def event(stage):
    print(json.dumps({"protocol": 1, "stage": stage}), flush=True)


def annotate_model(result, metadata):
    """Keep the model family separate from its release version; never invent legacy identity."""
    name = metadata.get('model_name')
    if name is not None:
        if not isinstance(name, str) or not name.strip() or len(name) > 200 or any(ord(c) < 32 for c in name):
            raise ValueError('invalid_model_identity')
        if result.get('model') not in (None, name):
            raise ValueError('model_identity_mismatch')
        result['model'] = name
    return result


def read_audio(path):
    import numpy as np
    from scipy.signal import resample_poly
    with wave.open(str(path), "rb") as wav:
        if wav.getcomptype() != "NONE" or wav.getsampwidth() not in (1, 2, 3, 4):
            raise ValueError("unsupported_audio")
        channels, rate = wav.getnchannels(), wav.getframerate()
        if not 1 <= channels <= 8 or not 8000 <= rate <= 192000:
            raise ValueError("unsupported_audio")
        width = wav.getsampwidth()
        raw = wav.readframes(wav.getnframes())
    if width == 1:
        audio = (np.frombuffer(raw, np.uint8).astype(np.float32) - 128) / 128
    elif width == 3:
        b = np.frombuffer(raw, np.uint8).reshape(-1, 3).astype(np.int32)
        audio = (b[:, 0] | b[:, 1] << 8 | b[:, 2] << 16)
        audio = ((audio ^ 0x800000) - 0x800000).astype(np.float32) / 8388608
    else:
        audio = np.frombuffer(raw, '<i2' if width == 2 else '<i4').astype(np.float32) / (2 ** (8 * width - 1))
    audio = audio.reshape(-1, channels).mean(axis=1)
    if len(audio) < rate / 40 or not np.isfinite(audio).all():
        raise ValueError("unsupported_audio")
    return audio, rate


def load_audio(path, noise=None):
    import numpy as np
    from scipy.signal import resample_poly
    audio, rate = read_audio(path)
    if noise is not None:
        return noise.prepare(audio, rate)
    if rate != 16000:
        divisor = math.gcd(rate, 16000)
        audio = resample_poly(audio, 16000 // divisor, rate // divisor).astype(np.float32)
    if len(audio) < 400 or not np.isfinite(audio).all():
        raise ValueError("unsupported_audio")
    return audio


def protect_offline():
    # No hub access, telemetry, dynamic code fetches, or accidental network fallback.
    os.environ.update(ORT_DISABLE_TELEMETRY="1", HF_HUB_OFFLINE="1", TRANSFORMERS_OFFLINE="1", HF_HUB_DISABLE_TELEMETRY="1",
                      TOKENIZERS_PARALLELISM="false", PYTHONDONTWRITEBYTECODE="1")
    import socket
    def deny_network(*args, **kwargs):
        raise OSError("Network access is disabled in the ASR worker")
    socket.socket.connect = deny_network
    socket.create_connection = deny_network
    from provider import prepare_private_runtime, prepare_stdin
    prepare_private_runtime()
    prepare_stdin()


def run(bundle, audio_path, output, noise_config=None):
    protect_offline()
    event("Preparing recording")
    with contextlib.redirect_stdout(sys.stderr):
        noise = None
        if noise_config is not None:
            from noise_reduction import NoiseReducer
            noise = NoiseReducer(json.loads(noise_config.read_text()))
            event("Reducing background noise")
        audio = load_audio(audio_path, noise)
    event("Loading Amharic voice tools")
    with contextlib.redirect_stdout(sys.stderr):
        from provider import load_provider
        asr, _ = load_provider(bundle, 'transcription')
    event("Recognizing and timing words")
    with contextlib.redirect_stdout(sys.stderr):
        result = asr.transcribe(audio)
    metadata = {'model_name': asr._studio_model_name}
    version = asr.version
    if result.get('model_version') != version:
        raise ValueError('model_version_mismatch')
    annotate_model(result, metadata)
    if noise is not None:
        result['processing'] = {'noise_reduction': noise.metadata()}
    duration = len(audio) / 16000
    for word in result['words']:
        if not (math.isfinite(word['start']) and math.isfinite(word['end']) and
                0 <= word['start'] <= word['end'] <= duration + 0.04):
            raise ValueError('invalid_alignment')
    pending = output.with_suffix('.pending')
    with pending.open('w', encoding='utf-8') as file:
        json.dump(result, file, ensure_ascii=False, allow_nan=False)
        file.flush()
        os.fsync(file.fileno())
    os.replace(pending, output)
    event("Saving transcript")


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--bundle', type=Path, required=True)
    parser.add_argument('--audio', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--noise-config', type=Path)
    args = parser.parse_args()
    try:
        # The managed parent keeps stdin open. Bootstrap private DLLs before
        # starting its reader: loading a CRT while another thread reads it can
        # deadlock during Windows DLL initialization.
        protect_offline()
        if os.environ.get('ETHIOPIC_MANAGED_WORKER') == '1':
            import threading
            from provider import read_stdin
            def watch_parent():
                try:
                    while read_stdin(1):
                        pass
                finally:
                    os._exit(1)
            threading.Thread(target=watch_parent, daemon=True).start()
        run(args.bundle, args.audio, args.output, args.noise_config)
    except Exception as exc:
        if os.environ.get('ETHIOPIC_ASR_DEVELOPER_DIAGNOSTICS') == '1':
            import traceback
            traceback.print_exc(file=sys.stderr)
        # Do not disclose content, filesystem paths, or tracebacks to the UI/logs.
        code = ('out_of_memory' if isinstance(exc, MemoryError) or 'out of memory' in str(exc).lower()
                else str(exc) if str(exc) in {'gpu_unavailable', 'gpu_precision_unsupported'} else 'worker_failed')
        print(json.dumps({'protocol': 1, 'error': code}), flush=True)
        sys.exit(1)
