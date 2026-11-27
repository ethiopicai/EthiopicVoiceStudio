"""Optional offline recognition preprocessing. Original audio never enters output storage.

One resident model, a new causal session and resampler for each recording. Native
setup supplies only verified local package/checkpoint paths; no hub resolution.
"""
import time
from pathlib import Path
import sys

import numpy as np

RATE = 48000
ASR_RATE = 16000


def checked_audio(value):
    value = np.asarray(value, dtype=np.float32)
    if value.ndim != 1 or not np.isfinite(value).all():
        raise ValueError('noise_audio_invalid')
    return value


class Downsampler:
    """48→16 kHz low-pass FIR, with continuous state and compensated filter delay."""
    def __init__(self):
        from scipy.signal import firwin
        self.kernel = firwin(61, 1 / 3, window=('kaiser', 5.0))
        self.state = np.zeros(60, np.float64)
        self.input_count = self.output_count = self.filtered = 0
        self.closed = False

    def _filter(self, audio):
        from scipy.signal import lfilter
        result, self.state = lfilter(self.kernel, [1.0], audio, zi=self.state)
        indices = np.arange(self.filtered, self.filtered + len(result))
        self.filtered += len(result)
        result = result[(indices >= 30) & (indices % 3 == 0)]
        self.output_count += len(result)
        return result.astype(np.float32)

    def push(self, audio):
        if self.closed:
            raise ValueError('noise_session_closed')
        audio = checked_audio(audio)
        self.input_count += len(audio)
        return self._filter(audio) if len(audio) else np.empty(0, np.float32)

    def finish(self):
        if self.closed:
            raise ValueError('noise_session_closed')
        self.closed = True
        expected = (self.input_count + 1) // 3
        remaining = expected - self.output_count
        result = self._filter(np.zeros(33, np.float32))[:remaining]
        if len(result) != remaining:
            raise ValueError('noise_duration_mismatch')
        return result


class Session:
    def __init__(self, model):
        self.stream = model.stream()
        self.downsample = Downsampler()
        self.input_count = self.isolated_count = self.output_count = 0
        self.closed = False

    def push(self, audio):
        if self.closed:
            raise ValueError('noise_session_closed')
        audio = checked_audio(audio)
        self.input_count += len(audio)
        isolated = checked_audio(self.stream.push(audio))
        self.isolated_count += len(isolated)
        result = self.downsample.push(isolated)
        self.output_count += len(result)
        return result

    def finish(self):
        if self.closed:
            raise ValueError('noise_session_closed')
        self.closed = True
        isolated = checked_audio(self.stream.flush(as_numpy=True))
        self.isolated_count += len(isolated)
        if self.isolated_count != self.input_count:
            raise ValueError('noise_duration_mismatch')
        result = np.concatenate([self.downsample.push(isolated), self.downsample.finish()])
        self.output_count += len(result)
        if self.output_count != (self.input_count + 1) // 3:
            raise ValueError('noise_duration_mismatch')
        return result


class NoiseReducer:
    def __init__(self, config, factory=None):
        if not isinstance(config, dict) or set(config) != {'site', 'checkpoint', 'device', 'revision'}:
            raise ValueError('noise_configuration_invalid')
        if config['device'] not in ('cpu', 'mps') or config['revision'] != '9b0285b3fe8403aa8e2f1dbb2165a80a4294c970':
            raise ValueError('noise_configuration_invalid')
        self.config = config.copy()
        self.factory = factory
        self.model = None

    def load(self):
        if self.model is None:
            if self.factory is None:
                checkpoint, site = Path(self.config['checkpoint']), Path(self.config['site'])
                if not checkpoint.is_file() or not site.is_dir():
                    raise ValueError('noise_tools_missing')
                sys.path.insert(0, str(site))
                from inworld.voice_isolation import VoiceIsolator
                self.factory = VoiceIsolator
            self.model = self.factory(self.config['checkpoint'], mode='streaming', block_frames=16,
                                      lookahead_frames=2, device=self.config['device'], precision='fp32')
        return self.model

    def begin(self):
        return Session(self.load())

    def prepare(self, audio, sample_rate, progress=lambda _: None):
        from scipy.signal import resample_poly
        import math
        audio = checked_audio(audio)
        expected = int(np.floor(len(audio) * ASR_RATE / sample_rate + .5))
        if sample_rate != RATE:
            divisor = math.gcd(sample_rate, RATE)
            audio = resample_poly(audio, RATE // divisor, sample_rate // divisor).astype(np.float32)
        session = self.begin()
        output = []
        for start in range(0, len(audio), RATE // 2):
            output.append(session.push(audio[start:start + RATE // 2]))
            progress(min(100, round((start + RATE // 2) / len(audio) * 100)))
        output.append(session.finish())
        result = checked_audio(np.concatenate(output))
        if abs(len(result) - expected) > 1:
            raise ValueError('noise_duration_mismatch')
        return np.pad(result[:expected], (0, max(0, expected - len(result))))

    def metadata(self):
        return {'provider': 'inworld-voice-isolator-v1-realtime', 'revision': self.config['revision'],
                'sha256': '76598f05523c0669e76562a8b2f31c9d395fca316e80c907ee11775e7497ae0f',
                'adapter_version': 1, 'lookahead_ms': 20, 'device': self.config['device']}

    def benchmark(self, seconds=3):
        # Warm shaders/model separately so first-use compilation is not measured
        # as steady-state live performance. The captured source is never used.
        self.prepare(np.zeros(RATE, np.float32), RATE)
        session = self.begin()
        started = time.monotonic()
        for _ in range(seconds * 2):
            session.push(np.zeros(RATE // 2, np.float32))
        session.finish()
        return (time.monotonic() - started) / seconds
