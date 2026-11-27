"""Torch-free rolling CTC stream, matching the released StreamingTranscriber API.

The released algorithm only requires decode(audio, offset). Keeping that loop
independent of its Torch module lets ONNX CPU models use identical commit rules.
"""
import numpy as np

SAMPLE_RATE = 16000


class StreamingTranscriber:
    def __init__(self, asr, update_sec=.4, commit_sec=2., pause_sec=.6,
                 context_sec=5., max_utt_sec=20.):
        self.asr = asr
        self.update = int(update_sec * SAMPLE_RATE)
        self.commit, self.pause = commit_sec, pause_sec
        self.context, self.max_utt = context_sec, max_utt_sec
        self.reset()

    def reset(self):
        self.buf = np.zeros(0, np.float32)
        self.start = self.pending = 0
        self.done = 0.

    def feed(self, samples):
        self.buf = np.concatenate([self.buf, np.asarray(samples, dtype=np.float32)])
        self.pending += len(samples)
        if self.pending < self.update:
            return None
        self.pending = 0
        words, end = self._decode()
        if not words:
            self._finalise(max(self.done, end - self.pause))
            return {'final': [], 'partial': []}
        if end - words[-1]['end'] >= self.pause or end - self.done >= self.max_utt:
            self._finalise((words[-1]['end'] + end) / 2)
            return {'final': words, 'partial': []}
        n = sum(word['end'] <= end - self.commit for word in words)
        if n:
            next_start = words[n]['start'] if n < len(words) else end
            self._finalise((words[n - 1]['end'] + next_start) / 2)
        return {'final': words[:n], 'partial': words[n:]}

    def finish(self):
        words, _ = self._decode()
        self.reset()
        return {'final': words, 'partial': []}

    def _decode(self):
        words = self.asr.decode(self.buf, self.start / SAMPLE_RATE)
        return ([word for word in words if (word['start'] + word['end']) / 2 > self.done],
                (self.start + len(self.buf)) / SAMPLE_RATE)

    def _finalise(self, time):
        self.done = max(self.done, time)
        n = min(max(int(round((time - self.context) * SAMPLE_RATE)) - self.start, 0), len(self.buf))
        self.buf, self.start = self.buf[n:], self.start + n
