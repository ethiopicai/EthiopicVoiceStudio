"""Audio preprocessing tests; run with the prepared private runtime (no checkpoint needed)."""
import tempfile
import unittest
import wave
from pathlib import Path
import numpy as np
from worker import load_audio, annotate_model


class AudioTests(unittest.TestCase):
    def test_model_family_is_explicit_and_conflicting_identity_is_rejected(self):
        for name in ['amharic-ctc', 'amharic-ctc-300m']:
            result = annotate_model({'model_version': 'v0.3'}, {'model_name': name})
            self.assertEqual(result['model'], name)
            self.assertEqual(result['model_version'], 'v0.3')
        self.assertNotIn('model', annotate_model({'model_version': 'legacy'}, {}))
        with self.assertRaisesRegex(ValueError, 'model_identity_mismatch'):
            annotate_model({'model': 'different'}, {'model_name': 'amharic-ctc'})
    def fixture(self, width, channels=1, rate=16000):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        path = Path(directory.name) / 'audio.wav'
        with wave.open(str(path), 'wb') as wav:
            wav.setparams((channels, width, rate, 0, 'NONE', 'not compressed'))
            # Zero PCM amplitude (8-bit PCM is unsigned).
            wav.writeframes((b'\x80' if width == 1 else b'\0' * width) * (rate // 10) * channels)
        return path

    def test_pcm_widths_and_downmix(self):
        for width in [1, 2, 3, 4]:
            with self.subTest(width=width):
                audio = load_audio(self.fixture(width, channels=2))
                self.assertEqual(audio.dtype, np.float32)
                self.assertEqual(len(audio), 1600)
                np.testing.assert_array_equal(audio, 0)

    def test_resampling_preserves_duration(self):
        self.assertEqual(len(load_audio(self.fixture(2, rate=48000))), 1600)

    def test_invalid_recording_is_rejected(self):
        with self.assertRaises(ValueError):
            load_audio(self.fixture(2, rate=4000))


if __name__ == '__main__':
    unittest.main()
