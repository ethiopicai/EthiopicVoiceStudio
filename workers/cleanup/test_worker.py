import tempfile
import unittest
import wave
from pathlib import Path
from unittest.mock import patch
import numpy as np
from worker import read_pcm, chunk_ranges, scaled_stft, enhance_recording


class WorkerTests(unittest.TestCase):
    def test_pcm_import_keeps_stereo_and_rate(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'input.wav'
            with wave.open(str(path), 'wb') as file:
                file.setparams((2, 2, 16000, 0, 'NONE', 'not compressed'))
                file.writeframes(np.tile(np.array([1000, -2000], dtype='<i2'), 1600).tobytes())
            audio, rate = read_pcm(path)
            self.assertEqual(audio.shape, (2, 1600))
            self.assertEqual(rate, 16000)
            np.testing.assert_allclose(audio[:, 0], [1000/32768, -2000/32768])

    def test_chunk_ranges_cover_tail_without_gaps(self):
        for frames in [1, 99, 100, 101, 213, 1000]:
            covered = np.zeros(frames)
            for start, end in chunk_ranges(frames, 100, 50):
                self.assertLessEqual(end-start, 100)
                covered[start:end] += 1
            self.assertTrue((covered > 0).all())

    def test_overlap_add_preserves_length_and_has_no_edge_gain(self):
        # An identity model fixture isolates the overlap-add code from ML quality.
        audio = np.ones((2, 1017), dtype=np.float32) * .1
        with patch('worker.enhance_chunk', side_effect=lambda model, source, config, rate: source):
            result = enhance_recording(None, {}, audio, 100, chunk_seconds=2)
        self.assertEqual(result.shape, audio.shape)
        np.testing.assert_allclose(result, audio, atol=1e-7)

    def test_scaled_fft_matches_original_release_rules(self):
        config = {'stft_cfg': {'n_fft': 320, 'hop_size': 40, 'win_size': 320, 'sampling_rate': 8000}}
        self.assertEqual(scaled_stft(config, 16000), (640, 80, 640))
        self.assertEqual(scaled_stft(config, 44100), (1764, 220, 1764))


if __name__ == '__main__':
    unittest.main()
