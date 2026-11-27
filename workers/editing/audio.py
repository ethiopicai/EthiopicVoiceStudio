"""PCM-preserving splice operations; only generated spans pass through an encoder."""
from dataclasses import dataclass
from pathlib import Path
import io
import wave
import numpy as np


@dataclass(frozen=True)
class PCM:
    rate: int
    channels: int
    width: int
    data: bytes

    @classmethod
    def read(cls, path):
        if Path(path).stat().st_size > 100 * 1024 * 1024:
            raise ValueError('Recording exceeds 100 MB')
        with wave.open(str(path), 'rb') as f:
            if (f.getcomptype() != 'NONE' or f.getsampwidth() not in (1, 2, 3, 4)
                    or not 1 <= f.getnchannels() <= 8 or not 8000 <= f.getframerate() <= 192000):
                raise ValueError('Use an uncompressed integer PCM WAV recording')
            frames = f.getnframes()
            result = cls(f.getframerate(), f.getnchannels(), f.getsampwidth(), f.readframes(frames))
            if len(result.data) != frames * result.stride:
                raise ValueError('Recording is truncated')
            return result

    @property
    def stride(self):
        return self.channels * self.width

    @property
    def frames(self):
        return len(self.data) // self.stride

    @property
    def duration(self):
        return self.frames / self.rate

    def slice(self, start, end):
        return PCM(self.rate, self.channels, self.width, self.data[start*self.stride:end*self.stride])

    def mono(self):
        if self.width == 1:
            values = np.frombuffer(self.data, np.uint8).astype(np.float32) - 128
        elif self.width == 3:
            b = np.frombuffer(self.data, np.uint8).reshape(-1, 3).astype(np.int32)
            values = b[:, 0] | (b[:, 1] << 8) | (b[:, 2] << 16)
            values = (values ^ 0x800000) - 0x800000
        else:
            values = np.frombuffer(self.data, '<i2' if self.width == 2 else '<i4')
        return (values.astype(np.float32).reshape(-1, self.channels).mean(axis=1)
                / (2 ** (8*self.width-1)))

    def encode(self, mono):
        limit = 2 ** (8*self.width-1)
        values = np.rint(np.clip(mono, -1, 1) * limit).clip(-limit, limit-1).astype(np.int64)
        values = np.repeat(values[:, None], self.channels, axis=1).reshape(-1)
        if self.width == 1:
            return (values+128).astype(np.uint8).tobytes()
        if self.width == 3:
            v = values & 0xffffff
            return np.stack((v & 255, (v >> 8) & 255, (v >> 16) & 255), axis=1).astype(np.uint8).tobytes()
        return values.astype('<i2' if self.width == 2 else '<i4').tobytes()

    def write(self, path):
        with wave.open(str(path), 'wb') as f:
            f.setparams((self.channels, self.width, self.rate, 0, 'NONE', 'not compressed'))
            f.writeframes(self.data)


def resample(values, source_rate, target_rate):
    if source_rate == target_rate:
        return np.asarray(values, np.float32)
    from scipy.signal import resample_poly
    from math import gcd
    common = gcd(source_rate, target_rate)
    return resample_poly(values, target_rate // common, source_rate // common).astype(np.float32)


def observe_splices(editor):
    """Observe exact sample changes inside the pinned API, before its whole-clip gain undo.

    Public span_in is rounded before 40 ms quantization; span_out excludes 20 ms fades
    and deletion seam regeneration. Never use those approximate ranges for PCM copying.
    No model file is modified. This adapter requires the pinned _edit_one interface.
    """
    original = editor._edit_one
    events = []

    def tracked(before, *args, **kwargs):
        result, metadata = original(before, *args, **kwargs)
        n = min(len(before), len(result))
        different = np.flatnonzero(before[:n].view(np.uint32) != result[:n].view(np.uint32))
        start = int(different[0]) if len(different) else n
        remaining = min(len(before)-start, len(result)-start)
        suffix = 0
        if remaining:
            changed = np.flatnonzero(before[-remaining:][::-1].view(np.uint32)
                                     != result[-remaining:][::-1].view(np.uint32))
            suffix = int(changed[0]) if len(changed) else remaining
        delta = len(result)-len(before)
        for event in events:
            event['output'] = [v+delta for v in event['output']]
        events.append({'input': [start, len(before)-suffix],
                       'output': [start, len(result)-suffix]})
        return result, metadata

    editor._edit_one = tracked
    return original, events


def splice_result(source, result, events):
    """Return PCM plus exact old/new frame ranges, keeping source bytes outside edits."""
    data = source.data
    marks = []
    for event in sorted(events, key=lambda e:e['input'][0], reverse=True):
        a, b = [max(0, min(source.frames, round(v*source.rate/24000))) for v in event['input']]
        c, d = event['output']
        generated = resample(result['audio'][c:d], 24000, source.rate)
        encoded = source.encode(generated)
        delta = len(generated)-(b-a)
        data = data[:a*source.stride] + encoded + data[b*source.stride:]
        for mark in marks:
            mark['span_out'] = [v+delta/source.rate for v in mark['span_out']]
            mark['frames_out'] = [v+delta for v in mark['frames_out']]
        marks.append({'span_in':[a/source.rate,b/source.rate],
                      'span_out':[a/source.rate,(a+len(generated))/source.rate],
                      'frames_in':[a,b], 'frames_out':[a,a+len(generated)]})
    return PCM(source.rate,source.channels,source.width,data), sorted(marks,key=lambda m:m['span_in'][0])


def preserved(before, after, marks):
    """Assert every untouched PCM byte range is identical, including stereo/24-bit."""
    a = b = 0
    for mark in marks:
        i,j = mark['frames_in']; k,l = mark['frames_out']
        if before.data[a*before.stride:i*before.stride] != after.data[b*after.stride:k*after.stride]:
            return False
        a,b = j,l
    return before.data[a*before.stride:] == after.data[b*after.stride:]
