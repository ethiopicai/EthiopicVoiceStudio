import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('windows_voice', Path(__file__).resolve().parents[1] / 'prepare-windows-voice-runtime.py')
voice = importlib.util.module_from_spec(spec)
spec.loader.exec_module(voice)


class WindowsVoiceRuntime(unittest.TestCase):
    def test_interpreter_copy_keeps_previous_runtime_unchanged_and_checks_crt(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            base = root / 'prior'
            python = base / 'runtime/python/python.exe'
            python.parent.mkdir(parents=True)
            python.write_bytes(b'private interpreter')
            payload = b'pinned runtime DLL'
            dll = python.parent / 'msvcp140.dll'
            dll.write_bytes(payload)
            pin = root / 'workers/asr/windows-vc-runtime.lock.json'
            pin.parent.mkdir(parents=True)
            pin.write_text(json.dumps({'files': [{'path': dll.name, 'bytes': len(payload),
                                                  'sha256': hashlib.sha256(payload).hexdigest()}]}))
            destination = root / 'new'
            with patch.object(voice, 'ROOT', root):
                copied = voice.copy_interpreter(base, destination)
                self.assertEqual(copied.read_bytes(), python.read_bytes())
                self.assertEqual((copied.parent / dll.name).read_bytes(), payload)
                with self.assertRaises(ValueError):
                    voice.copy_interpreter(base, destination)
                dll.write_bytes(b'tampered')
                with self.assertRaises(ValueError):
                    voice.copy_interpreter(base, root / 'bad')
            self.assertFalse((root / 'bad').exists())

    def test_selected_models_only_are_prepared_and_exact_manifest_files_preserved(self):
        for provider, expected in [('small-tts', ['tts-flow_lm-90m-amh-fp32']), ('editing', [
                'edit-codec_llm-0.6b-amh', 'tts-codec_llm-0.6b-amh-fp16', 'codec-higgs_audio_v2-fp32', 'asr-wav2vec_ctc-1b-amh-int8_onnx'])]:
            with self.subTest(provider=provider), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                variants = []
                for identity in expected + ['unselected-model']:
                    content = identity.encode()
                    variants.append({'id': identity, 'folder': identity, 'files': [{
                        'path': 'fixture.bin', 'bytes': len(content), 'sha256': hashlib.sha256(content).hexdigest()}]})
                downloaded = []
                def fetch(manifest, variant, cache):
                    downloaded.append(variant['id'])
                    folder = cache / variant['folder']
                    folder.mkdir(parents=True)
                    (folder / 'fixture.bin').write_bytes(variant['id'].encode())
                    return folder
                def valid(path, entry):
                    return path.is_file() and hashlib.sha256(path.read_bytes()).hexdigest() == entry['sha256']
                release = SimpleNamespace(fetch_variant=fetch, valid_file=valid)
                manifest = {'variants': variants, 'hf_revision': 'pinned'}
                with patch.object(voice, 'script', return_value=release):
                    voice.prepare_models(manifest, provider, root / 'models', root / 'cache')
                self.assertEqual(downloaded, expected)
                self.assertFalse((root / 'models/unselected-model').exists())
                self.assertEqual(json.loads((root / 'models/manifest.json').read_text()), manifest)
                for identity in expected:
                    self.assertEqual((root / 'models' / identity / 'fixture.bin').read_bytes(), identity.encode())


if __name__ == '__main__':
    unittest.main()
