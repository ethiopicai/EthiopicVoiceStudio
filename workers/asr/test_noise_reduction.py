import importlib.util
from pathlib import Path
import tempfile
import unittest
import zipfile
import numpy as np

from noise_reduction import Downsampler, NoiseReducer
from noise_setup import extract
from dictation_worker import AudioQueue, consume

CONFIG = {'site': 'test-site', 'checkpoint': 'test-model', 'device': 'cpu',
          'revision': '9b0285b3fe8403aa8e2f1dbb2165a80a4294c970'}


class DelayedStream:
    def __init__(self):
        self.pending = np.empty(0, np.float32)

    def push(self, audio):
        self.pending = np.concatenate([self.pending, audio])
        count = max(0, len(self.pending) - 3008)
        out = self.pending[:count].copy()
        self.pending = self.pending[count:]
        return out

    def flush(self, as_numpy=False):
        out = self.pending
        self.pending = np.empty(0, np.float32)
        return out


class FakeModel:
    def stream(self):
        return DelayedStream()


class NoiseTests(unittest.TestCase):
    def test_downsample_boundaries_delay_tail_and_frequency_response(self):
        for length in [0, 1, 2, 3, 31, 1000, 48001]:
            x = np.random.default_rng(4).normal(0, .1, length).astype(np.float32)
            expected = np.convolve(x, Downsampler().kernel)[30:30 + length:3][:(length + 1)//3] if length else np.empty(0)
            for size in [1, 317, 960, 24000]:
                reducer = Downsampler()
                parts = [reducer.push(x[i:i+size]) for i in range(0, length, size)]
                parts.append(reducer.finish())
                out = np.concatenate(parts)
                self.assertEqual(len(out), (length + 1)//3)
                np.testing.assert_allclose(out, expected, atol=1e-7)
                with self.assertRaises(ValueError):
                    reducer.push(np.zeros(1, np.float32))
        for frequency, limit in [(440, .72), (12000, .01)]:
            x = np.sin(np.arange(48000) * 2 * np.pi * frequency / 48000).astype(np.float32)
            reducer = Downsampler();out = np.concatenate([reducer.push(x), reducer.finish()])[100:-100]
            rms = np.sqrt(np.mean(out**2))
            self.assertLess(rms, limit)
            if frequency == 440:
                self.assertGreater(rms, .69)

    def test_model_is_loaded_once_sessions_and_duration_are_independent(self):
        loads = []
        def factory(*args, **kwargs):
            loads.append((args, kwargs));return FakeModel()
        model = NoiseReducer(CONFIG, factory=factory)
        for n in [1200, 48000, 48001]:
            source = np.random.default_rng(2).normal(0, .01, n).astype(np.float32)
            saved = source.copy();session = model.begin();parts = []
            for start in range(0, n, 317):
                parts.append(session.push(source[start:start+317]))
            parts.append(session.finish());out = np.concatenate(parts)
            self.assertEqual(len(out), (n + 1)//3)
            np.testing.assert_array_equal(source, saved)
            with self.assertRaises(ValueError):session.finish()
        self.assertEqual(len(loads), 1)
        self.assertEqual(loads[0][1]['mode'], 'streaming')
        self.assertEqual(loads[0][1]['lookahead_frames'], 2)

    def test_rate_aware_queue_flush_reaches_asr_before_finish(self):
        queue = AudioQueue(48000)
        queue.push([.1] * 1200);queue.finish()
        class ASR:
            def __init__(self):self.samples=0
            def feed(self, audio):self.samples+=len(audio);return None
            def finish(self):
                return {'final':[{'word':'tail','start':0,'end':self.samples/16000}], 'partial':[]}
        asr = ASR();events=[]
        words,count = consume(asr, queue, lambda **e:events.append(e), NoiseReducer(CONFIG, factory=lambda *a, **k:FakeModel()).begin())
        self.assertEqual(count, 400)
        self.assertEqual(words[0]['end'], .025)
        self.assertEqual(events[-1]['audio_seconds'], .025)
        with self.assertRaises(ValueError):AudioQueue(44100)
        bounded=AudioQueue(48000)
        for _ in range(5):bounded.push([0.] * 96000)
        with self.assertRaisesRegex(ValueError,'backlog_exceeded'):bounded.push([0.])
        with self.assertRaises(ValueError):AudioQueue(48000).push([0.] * 96001)
        with self.assertRaises(ValueError):NoiseReducer({**CONFIG, 'device':'invalid'})
        with self.assertRaises(ValueError):NoiseReducer(CONFIG, factory=lambda *a, **k:FakeModel()).begin().push([np.nan])

    def test_wheel_overlay_rejects_paths_links_and_startup_hooks(self):
        for name, mode in [('../outside',0),('/absolute',0),('a\\b',0),('startup.pth',0),('link',0o120777)]:
            with tempfile.TemporaryDirectory() as directory:
                root=Path(directory);wheel=root/'input.whl'
                with zipfile.ZipFile(wheel,'w') as archive:
                    info=zipfile.ZipInfo(name);info.external_attr=mode<<16;archive.writestr(info,b'bad')
                with self.assertRaises(ValueError):extract([wheel],root/'site')
                self.assertFalse((root/'outside').exists())


if __name__ == '__main__':
    unittest.main()
