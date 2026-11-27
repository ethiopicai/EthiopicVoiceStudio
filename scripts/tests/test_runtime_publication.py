import copy
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('runtime_publication', Path(__file__).resolve().parents[1] / 'publish-asr-runtime-packs.py')
publication = importlib.util.module_from_spec(spec)
spec.loader.exec_module(publication)


class RuntimePublication(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        packs = []
        for kind in ('model', 'runtime'):
            path = self.root / f'test-{kind}.tar.gz'
            path.write_bytes(kind.encode())
            packs.append(dict(kind=kind, filename=path.name, sha256=publication.digest(path), size=path.stat().st_size, unpacked_size=100))
        self.release = dict(os='windows', arch='x86_64', minimum_windows='10.0.19041', profile='cpu-low',
                            model_version='v0.3', hf_revision='a' * 40,
                            transcription='asr-wav2vec_ctc-300m-amh-int8_onnx', dictation='asr-wav2vec_ctc-300m-amh-int8_onnx', packs=packs)
        (self.root / 'pack-build-windows.json').write_text(json.dumps(dict(schema_version=1, releases=[self.release])))
        self.evidence = dict(schema_version=1, profiles={'cpu-low': dict(offline_inference=True, guided_install=True,
                             pack_sha256={p['filename']: p['sha256'] for p in packs})})

    def test_only_exact_native_qualified_bytes_can_be_published(self):
        metadata, archives = publication.qualified_archives(self.root, self.evidence)
        self.assertEqual(len(archives), 2)
        for field in ('offline_inference', 'guided_install', 'pack_sha256'):
            bad = copy.deepcopy(self.evidence)
            bad['profiles']['cpu-low'].pop(field)
            with self.assertRaises(ValueError):
                publication.qualified_archives(self.root, bad)
        (self.root / self.release['packs'][0]['filename']).write_bytes(b'other')
        with self.assertRaises(ValueError):
            publication.qualified_archives(self.root, self.evidence)

    def test_generated_catalog_keeps_model_revision_separate_from_runtime_revision(self):
        catalog, routes = publication.catalogs({'releases': [self.release]}, 'b' * 40)
        self.assertEqual(routes['releases'][0]['hf_revision'], 'a' * 40)
        self.assertIn('/resolve/' + 'b' * 40 + '/', catalog['releases'][0]['packs'][0]['url'])
        self.assertEqual(catalog['releases'][0]['minimum_windows'], '10.0.19041')
        for invalid in ('main', 'latest', 'a' * 39):
            with self.assertRaises(ValueError):
                publication.catalogs({'releases': [self.release]}, invalid)

    def test_archive_names_cannot_escape_the_publication_allowlist(self):
        for name in ('../private.tar.gz', 'runtime.zip', '_work_/runtime.tar.gz'):
            bad = copy.deepcopy(self.release)
            bad['packs'][0]['filename'] = name
            (self.root / 'pack-build-windows.json').write_text(json.dumps(dict(schema_version=1, releases=[bad])))
            with self.assertRaises(ValueError):
                publication.qualified_archives(self.root, self.evidence)


if __name__ == '__main__':
    unittest.main()
