import importlib.util
import json
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest.mock import patch


def load(name):
    path = Path(__file__).resolve().parents[1] / (name + '.py')
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


release = load('prepare-windows-asr-release')
runtime = load('prepare-windows-asr-runtime')


class WindowsRecognitionPacks(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)

    def manifest(self):
        path = Path(__file__).resolve().parents[2] / 'apps/desktop/resources/studio-manifest.json'
        return release.checked_manifest(path)

    def test_cpu_and_gpu_profiles_keep_exact_published_model_selections(self):
        manifest = self.manifest()
        for profile in release.PROFILES:
            config = release.recognition_config(manifest, profile)
            self.assertEqual(config['transcription']['variant'], manifest['bundles'][profile]['asr'])
            self.assertEqual(config['dictation']['variant'], manifest['bundles'][profile]['asr_live'])
        cpu = release.recognition_config(manifest, 'cpu-high')
        self.assertEqual(cpu['transcription']['backend'], 'onnx')
        self.assertEqual(cpu['dictation']['context_sec'], 2)
        gpu = release.recognition_config(manifest, 'gpu-modern')
        self.assertEqual(gpu['transcription']['device'], 'cuda')
        self.assertEqual(gpu['dictation']['dtype'], 'float16')

    def test_portable_windows_runtime_archive_is_reproducible_and_self_contained(self):
        source = self.root / 'source'
        interpreter = source / 'runtime/python/cpython-3.11-test/python.exe'
        interpreter.parent.mkdir(parents=True)
        interpreter.write_bytes(b'private Windows interpreter fixture')
        metadata = source / 'runtime/site-packages/example.dist-info/WHEEL'
        metadata.parent.mkdir(parents=True)
        metadata.write_text('Tag: cp311-cp311-win_amd64\n')
        runtime.validate_wheels(source)
        self.assertEqual(runtime.private_python(source), interpreter)
        output = self.root / 'output'
        first = release.build_pack(source, output, 'runtime', 'v0.3', 'onnx')
        second = release.build_pack(source, output, 'runtime', 'v0.3', 'onnx')
        self.assertEqual(first, second)
        with tarfile.open(output / first['filename']) as archive:
            pack = json.load(archive.extractfile('pack.json'))
            self.assertEqual(pack['python'], 'runtime/python/cpython-3.11-test/python.exe')
            self.assertEqual(archive.extractfile(pack['python']).read(), interpreter.read_bytes())
            self.assertEqual(first['unpacked_size'], sum(item.size for item in archive))
            self.assertTrue(all(item.isfile() for item in archive))

    def test_non_windows_wheels_and_escaping_links_are_rejected(self):
        source = self.root / 'source'
        metadata = source / 'runtime/site-packages/example.dist-info/WHEEL'
        metadata.parent.mkdir(parents=True)
        metadata.write_text('Tag: cp311-cp311-macosx_14_0_arm64\n')
        with self.assertRaisesRegex(ValueError, 'Not a Windows'):
            runtime.validate_wheels(source)
        outside = self.root / 'outside'
        outside.write_bytes(b'not a distributable file')
        try:
            (source / 'runtime/leak').symlink_to(outside)
        except OSError as error:
            if getattr(error, 'winerror', None) != 1314:
                raise
            self.skipTest('Windows symlink privilege is unavailable; exercised on macOS')
        with self.assertRaisesRegex(ValueError, 'leaves its pack'):
            release.build_pack(source, self.root / 'output', 'runtime', 'v0.3', 'onnx')

    def test_hf_files_are_pinned_and_paths_cannot_escape(self):
        data = self.manifest()
        path = self.root / 'manifest.json'
        data['hf_revision'] = 'main'
        path.write_text(json.dumps(data))
        with self.assertRaisesRegex(ValueError, 'immutable'):
            release.checked_manifest(path)
        for unsafe in ['../weights', '/tmp/weights', 'C:/weights', 'a\\weights', 'a//weights']:
            self.assertFalse(release.safe_path(unsafe))

    def test_generated_metadata_preserves_the_full_pinned_variant(self):
        cache = self.root / 'cache/model-fixture'
        cache.mkdir(parents=True)
        payload = cache / 'asr.int8.onnx'
        payload.write_bytes(b'verified model fixture')
        variant = {'id': 'asr-fixture-onnx-int8', 'folder': 'model-fixture', 'component': 'asr',
                   'model': 'amharic-ctc', 'model_version': 'v0.3', 'runtime': 'onnxruntime-cpu',
                   'precision': 'int8', 'files': [{'path': payload.name, 'bytes': payload.stat().st_size,
                                                 'sha256': release.digest(payload)}]}
        manifest = {'variants': [variant], 'bundles': {'cpu-low': {'asr': variant['id'], 'asr_live': variant['id']}}}
        source = self.root / 'staged'
        with patch.object(release, 'fetch_variant', return_value=cache):
            release.prepare_models(manifest, 'cpu-low', cache.parent, source)
        generated = json.loads((source / 'bundle/model-fixture/variant.json').read_text())
        self.assertEqual(generated, variant)
        self.assertFalse((cache / 'variant.json').exists(), 'Source model cache must stay unchanged')

    def test_private_crt_copy_checks_pinned_bytes_and_keeps_notices(self):
        source = self.root / 'source'
        interpreter = source / 'runtime/python/private/python.exe'
        interpreter.parent.mkdir(parents=True)
        interpreter.write_bytes(b'interpreter fixture')
        crt = self.root / 'crt'
        crt.mkdir()
        dll = crt / 'msvcp140.dll'
        dll.write_bytes(b'signed DLL fixture')
        for notice in ('NOTICE.txt', 'crt-provenance.json'):
            (crt / notice).write_text('retained redistribution notice')
        lock = {'files': [{'path': dll.name, 'bytes': dll.stat().st_size, 'sha256': release.digest(dll)}]}
        runtime.include_crt(source, crt, lock)
        self.assertEqual((interpreter.parent / dll.name).read_bytes(), dll.read_bytes())
        self.assertTrue((source / 'runtime/licenses/visual-cpp/NOTICE.txt').is_file())
        dll.write_bytes(b'tampered DLL fixture')
        with self.assertRaisesRegex(ValueError, 'pinned SHA-256'):
            runtime.include_crt(source, crt, lock)


if __name__ == '__main__':
    unittest.main()
