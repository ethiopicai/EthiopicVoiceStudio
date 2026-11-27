import importlib.util
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parent))
from audio import PCM

spec = importlib.util.spec_from_file_location('controlled_voice_worker', Path(__file__).with_name('worker.py'))
worker = importlib.util.module_from_spec(spec)
spec.loader.exec_module(worker)


class FakeTTS:
    def __init__(self):
        self.calls = []
        self.voice_calls = []
        self.fail_second = False

    def voice(self, path, text):
        self.voice_calls.append((path, text))
        return object()

    def synth(self, text, voice, **kwargs):
        self.calls.append((text, voice, kwargs))
        if self.fail_second and len(self.calls) == 2:
            return np.array([float('nan')], np.float32)
        count = round(kwargs['duration'] * 24000) if 'duration' in kwargs else 100
        return np.full(count, .02, np.float32)


class SpeechControls(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name)
        self.reference = self.root / 'reference.wav'
        PCM(24000, 1, 2, b'\0\0' * 24000).write(self.reference)
        self.reference_bytes = self.reference.read_bytes()
        manifest = {'hf_revision': 'pinned', 'variants': [{
            'id': 'asr-wav2vec_ctc-1b-amh-int8_onnx', 'model_version': 'v0.3'}]}
        self.provider = worker.Provider(self.root, self.root, manifest, 'tts-codec_llm-0.6b-amh-fp16', 32)
        self.tts = FakeTTS()
        self.provider.editor = SimpleNamespace(tts=self.tts)
        self.project = self.provider.projects.create(self.reference, 'Reference')
        self.request = {'action': 'synthesize', 'projectId': self.project['id'], 'revision': 0,
                        'seed': 42, 'text': 'ሰላም። ጤና ይስጥልኝ።', 'referenceText': 'ሰላም።'}
        self.output = self.provider.projects.folder(self.project['id']) / 'synthesis.wav'

    def test_temperature_speed_steps_and_sentence_silence_are_applied_and_voice_is_cached(self):
        options = {'temperature': .2, 'speed': .8, 'numSteps': 64, 'sentencePause': .5}
        result = self.provider.dispatch({**self.request, 'generationOptions': options})
        self.assertEqual(len(self.tts.calls), 2)
        for index, (text, voice, kwargs) in enumerate(self.tts.calls):
            self.assertEqual(kwargs, {'num_step': 64, 'class_temperature': .2, 'speed': .8, 'seed': 42 + index})
        self.assertIs(self.tts.calls[0][1], self.tts.calls[1][1])
        pcm = PCM.read(self.output)
        self.assertEqual(pcm.frames, 12200)
        self.assertEqual(pcm.data[200:24200], b'\0' * 24000)
        self.assertEqual(result['settings'], {'schemaVersion': 1, 'seed': 42, **options})
        self.provider.dispatch({**self.request, 'generationOptions': {**options, 'sentencePause': 0}})
        self.assertEqual(PCM.read(self.output).frames, 200)
        self.assertEqual(len(self.tts.voice_calls), 1)
        self.assertEqual(self.reference.read_bytes(), self.reference_bytes)

    def test_duration_uses_whole_script_model_timing_without_speed_pause_or_edge_padding(self):
        result = self.provider.dispatch({**self.request, 'generationOptions': {
            'temperature': .2, 'duration': 4, 'numSteps': 16}})
        text, _, kwargs = self.tts.calls[0]
        self.assertEqual(len(self.tts.calls), 1)
        self.assertEqual(text, self.request['text'])
        self.assertEqual(kwargs, {'num_step': 16, 'class_temperature': .2, 'seed': 42,
                                 'duration': 4., 'postprocess_output': False, 'pad_duration': 0.})
        self.assertNotIn('fade_duration', kwargs)
        self.assertNotIn('speed', result['settings'])
        self.assertNotIn('sentencePause', result['settings'])
        self.assertEqual(result['settings']['duration'], 4.)
        self.assertEqual(result['duration'], 4.)
        self.assertEqual(PCM.read(self.output).frames, 96000)

    def test_absent_options_preserve_old_whole_script_synthesis(self):
        result = self.provider.dispatch(self.request)
        self.assertEqual(len(self.tts.calls), 1)
        self.assertEqual(self.tts.calls[0][0], self.request['text'])
        self.assertEqual(self.tts.calls[0][2], {'num_step': 32, 'seed': 42})
        self.assertEqual(result['settings'], {'schemaVersion': 1, 'seed': 42,
                                             'temperature': 0., 'numSteps': 32, 'speed': 1.})

    def test_sentence_variation_wraps_at_maximum_u32_seed(self):
        result = self.provider.dispatch({**self.request, 'seed': 4294967295,
                                         'generationOptions': {'sentencePause': 0}})
        self.assertEqual([call[2]['seed'] for call in self.tts.calls], [4294967295, 0])
        self.assertEqual(result['settings']['seed'], 4294967295)

    def test_malformed_or_conflicting_controls_reject_before_loading_models(self):
        self.provider.editor = None
        with mock.patch.object(self.provider, 'load', side_effect=AssertionError('Model must not load')) as load:
            for options in [[], False, {'unknown': 1}, {'temperature': True}, {'temperature': None},
                            {'temperature': float('nan')}, {'temperature': -1}, {'temperature': 1.1},
                            {'speed': .49}, {'speed': 2.01}, {'speed': float('inf')}, {'duration': .49},
                            {'duration': 601}, {'duration': True}, {'numSteps': 8}, {'numSteps': 16.},
                            {'numSteps': True}, {'sentencePause': 2.1}, {'sentencePause': None},
                            {'duration': 3, 'speed': 1}, {'duration': 3, 'sentencePause': 0}]:
                with self.subTest(options=options), self.assertRaises(ValueError):
                    self.provider.dispatch({**self.request, 'generationOptions': options})
            for seed in [True, -1, 4294967296, .5]:
                with self.subTest(seed=seed), self.assertRaises(ValueError):
                    self.provider.dispatch({**self.request, 'seed': seed})
            load.assert_not_called()

    def test_second_sentence_failure_does_not_replace_audio_or_reference(self):
        self.output.write_bytes(b'saved take')
        self.tts.fail_second = True
        with self.assertRaises(ValueError):
            self.provider.dispatch({**self.request, 'generationOptions': {'sentencePause': .2}})
        self.assertEqual(self.output.read_bytes(), b'saved take')
        self.assertEqual(self.reference.read_bytes(), self.reference_bytes)
        self.assertFalse(self.output.with_suffix('.tmp').exists())

    def test_partial_file_write_failure_does_not_replace_saved_audio(self):
        self.output.write_bytes(b'saved take')
        def fail(path, *args, **kwargs):
            Path(path).write_bytes(b'incomplete new take')
            raise OSError('Disk full')
        with mock.patch('soundfile.write', side_effect=fail), self.assertRaises(OSError):
            self.provider.dispatch(self.request)
        self.assertEqual(self.output.read_bytes(), b'saved take')
        self.assertEqual(self.reference.read_bytes(), self.reference_bytes)
        self.assertFalse(self.output.with_suffix('.tmp').exists())


if __name__ == '__main__':
    unittest.main()
