"""Offline RE-USE enhancement with the validated CPU Mamba reference backend."""
import argparse
import contextlib
import json
import math
import os
from pathlib import Path
import sys
import types
import wave


PROTOCOL_OUT = sys.stdout

def event(stage):
    print(json.dumps({'protocol': 1, 'stage': stage}), file=PROTOCOL_OUT, flush=True)


def read_pcm(path):
    import numpy as np
    with wave.open(str(path), 'rb') as file:
        channels, rate, width = file.getnchannels(), file.getframerate(), file.getsampwidth()
        frames = file.getnframes()
        if file.getcomptype() != 'NONE' or channels not in (1, 2) or not 8000 <= rate <= 48000 or width not in (1, 2, 3, 4):
            raise ValueError('unsupported_audio')
        raw = file.readframes(frames)
    if len(raw) != frames * channels * width or frames < rate // 10:
        raise ValueError('unsupported_audio')
    if width == 1:
        audio = (np.frombuffer(raw, np.uint8).astype(np.float32) - 128) / 128
    elif width == 3:
        value = np.frombuffer(raw, np.uint8).reshape(-1, 3).astype(np.int32)
        value = value[:, 0] | value[:, 1] << 8 | value[:, 2] << 16
        audio = ((value ^ 0x800000) - 0x800000).astype(np.float32) / 8388608
    else:
        audio = np.frombuffer(raw, '<i2' if width == 2 else '<i4').astype(np.float32) / (2 ** (width * 8 - 1))
    return audio.reshape(-1, channels).T.copy(), rate


def chunk_ranges(frames, chunk, hop):
    if frames <= chunk:
        return [(0, frames)]
    starts = list(range(0, frames - chunk + 1, hop))
    if starts[-1] != frames - chunk:
        starts.append(frames - chunk)
    return [(start, min(start + chunk, frames)) for start in starts]


def scaled_stft(config, rate):
    def even(value):
        value = int(round(value))
        return value if value % 2 == 0 else value + 1
    stft = config['stft_cfg']
    return tuple(even(stft[name] * rate // stft['sampling_rate']) for name in ('n_fft', 'hop_size', 'win_size'))


def enhance_chunk(model, audio, config, rate):
    import torch
    from models.stfts import mag_phase_stft, mag_phase_istft
    n_fft, hop, window = scaled_stft(config, rate)
    # Same frontend, artifact mask, and reconstruction as the pinned NVIDIA release.
    magnitude, phase, _ = mag_phase_stft(audio, n_fft, hop, window, config['model_cfg']['compress_factor'], center=True, addeps=False)
    amplitude, phase, _ = model(magnitude, phase)
    mask = (torch.expm1(torch.relu(amplitude)) == 0).sum(1) / amplitude.shape[1]
    amplitude[:, :, (mask > .5)[0]] = 0
    result = mag_phase_istft(amplitude, phase, n_fft, hop, window, config['model_cfg']['compress_factor'])
    count = audio.shape[1]
    if result.shape[1] < count:
        result = torch.nn.functional.pad(result, (0, count - result.shape[1]), value=1e-8)
    return result[:, :count]


def load_model(bundle):
    import torch
    from safetensors.torch import load_file
    from mamba_reference import Mamba
    # Only the CUDA-specific Mamba execution backend is replaced. Official model
    # layers and all released weights are loaded without modification or hub calls.
    module = types.ModuleType('mamba_ssm')
    module.Mamba = Mamba
    sys.modules['mamba_ssm'] = module
    sys.path.insert(0, str(bundle))
    from models.generator_SEMamba_time_d4 import SEMamba
    config = json.loads((bundle / 'config.json').read_text())
    model = SEMamba(config)
    model.load_state_dict(load_file(str(bundle / 'model.safetensors'), device='cpu'), strict=True)
    return model.to(device='cpu', dtype=torch.float32).eval(), config


def enhance_recording(model, config, audio, rate, progress=None, chunk_seconds=2.0):
    import numpy as np
    import torch
    frames = audio.shape[1]
    chunk = int(chunk_seconds * rate)
    ranges = chunk_ranges(frames, chunk, max(1, chunk // 2))
    output = np.zeros(audio.shape, dtype=np.float32)
    normalization = np.zeros(frames, dtype=np.float32)
    for start, end in ranges:
        size = end - start
        weights = np.maximum(np.hanning(size + 1)[:-1], 1e-7).astype(np.float32) if len(ranges) > 1 else np.ones(size, dtype=np.float32)
        normalization[start:end] += weights
    count = 0
    total = len(ranges) * audio.shape[0]
    with torch.inference_mode():
        for channel in range(audio.shape[0]):
            for start, end in ranges:
                source = torch.from_numpy(audio[channel:channel + 1, start:end].copy())
                result = enhance_chunk(model, source, config, rate).cpu().numpy()[0]
                if not np.isfinite(result).all():
                    raise ValueError('invalid_output')
                size = end - start
                weights = np.maximum(np.hanning(size + 1)[:-1], 1e-7).astype(np.float32) if len(ranges) > 1 else np.ones(size, dtype=np.float32)
                output[channel, start:end] += result * weights
                count += 1
                if progress:
                    progress(count, total)
    output /= normalization[None, :]
    if output.shape != audio.shape or not np.isfinite(output).all():
        raise ValueError('invalid_output')
    return output


def run(bundle, audio_path, output, metadata):
    os.environ.update(HF_HUB_OFFLINE='1', TRANSFORMERS_OFFLINE='1', HF_HUB_DISABLE_TELEMETRY='1', TOKENIZERS_PARALLELISM='false')
    import socket
    def deny(*args, **kwargs):
        raise OSError('Network access is disabled in the enhancement worker')
    socket.socket.connect = deny
    socket.create_connection = deny
    event('Preparing recording')
    with contextlib.redirect_stdout(sys.stderr):
        import numpy as np
        import torch
        torch.set_num_threads(min(4, os.cpu_count() or 1))
        audio, rate = read_pcm(audio_path)
    event('Loading Voice Cleanup')
    with contextlib.redirect_stdout(sys.stderr):
        model, config = load_model(bundle)
    event('Cleaning your recording')
    total_blocks = len(chunk_ranges(audio.shape[1], 2 * rate, rate)) * audio.shape[0] * len(model.TSMamba)
    completed = 0
    def block_completed(*args):
        nonlocal completed
        completed += 1
        if completed % 3 == 0 or completed == total_blocks:
            event(f'Cleaning audio · {int(100 * completed / total_blocks)}%')
    handles = [block.register_forward_hook(block_completed) for block in model.TSMamba]
    # Model code must never emit user content through the protocol channel.
    with contextlib.redirect_stdout(sys.stderr):
        try:
            cleaned = enhance_recording(model, config, audio, rate)
        finally:
            for handle in handles:
                handle.remove()
    event('Checking cleaned recording')
    clipped = int(np.count_nonzero((cleaned < -1) | (cleaned > 1)))
    pcm = np.rint(np.clip(cleaned.T, -1, 32767 / 32768) * 32768).astype('<i2')
    pending = output.with_suffix('.pending')
    with wave.open(str(pending), 'wb') as wav:
        wav.setparams((audio.shape[0], 2, rate, 0, 'NONE', 'not compressed'))
        wav.writeframes(pcm.tobytes())
    with pending.open('rb') as file:
        os.fsync(file.fileno())
    os.replace(pending, output)
    version = json.loads((bundle / 'version.json').read_text())['model_version']
    result = {'model_version': version, 'sample_rate': rate, 'channels': audio.shape[0], 'frames': audio.shape[1],
              'backend': 'torch-cpu-reference', 'clipped_samples': clipped, 'chunk_seconds': 2.0}
    pending = metadata.with_suffix('.pending')
    with pending.open('w') as file:
        json.dump(result, file, allow_nan=False)
        file.flush()
        os.fsync(file.fileno())
    os.replace(pending, metadata)
    event('Saving cleaned recording')


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--bundle', type=Path, required=True)
    parser.add_argument('--audio', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--metadata', type=Path, required=True)
    args = parser.parse_args()
    if os.environ.get('ETHIOPIC_MANAGED_WORKER') == '1':
        import threading
        def watch_parent():
            while os.read(0, 1):
                pass
            os._exit(1)
        threading.Thread(target=watch_parent, daemon=True).start()
    try:
        run(args.bundle, args.audio, args.output, args.metadata)
    except Exception as exc:
        if os.environ.get('ETHIOPIC_CLEANUP_DEVELOPER_DIAGNOSTICS') == '1':
            import traceback
            traceback.print_exc(file=sys.stderr)
        code = 'out_of_memory' if isinstance(exc, MemoryError) or 'out of memory' in str(exc).lower() else 'worker_failed'
        print(json.dumps({'protocol': 1, 'error': code}), flush=True)
        sys.exit(1)
