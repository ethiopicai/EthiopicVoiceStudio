import contextlib
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import tarfile
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

spec=importlib.util.spec_from_file_location('editing_runtime',Path(__file__).resolve().parents[1]/'prepare-editing-runtime.py')
runtime=importlib.util.module_from_spec(spec);spec.loader.exec_module(runtime)


class EditingRuntimeArchive(unittest.TestCase):
    def test_payload_matches_native_extractor_contract(self):
        self.check_provider('editing')

    def test_cpu_tts_payload_uses_its_own_capability_and_descriptor(self):
        self.check_provider('small-tts')

    def test_load_only_probe_does_not_produce_a_release_archive(self):
        with tempfile.TemporaryDirectory() as d:
            root=Path(d);source=root/'source';models=root/'models';models.mkdir()
            (models/'manifest.json').write_text('{}')
            (source/'runtime/python').mkdir(parents=True)
            (source/'runtime/python/python.exe').write_bytes(b'interpreter fixture')
            (source/'runtime/site-packages').mkdir()
            args=['prepare-editing-runtime.py','--provider','small-tts','--source',str(source),
                  '--python','runtime/python/python.exe','--models',str(models),'--tts','tts-flow_lm-90m-amh-fp32','--output',str(root/'output')]
            result=SimpleNamespace(returncode=0,stdout='{"probe":true}\n',stderr='controlled qualification failure')
            with patch('sys.argv',args),patch.object(runtime.subprocess,'run',return_value=result),self.assertRaises(RuntimeError):
                runtime.main()
            self.assertFalse(list((root/'output').glob('*.tar.gz')))
            self.assertEqual((root/'output/probe-stderr.log').read_text(),'controlled qualification failure')

    def check_provider(self,provider):
        # The model probe is mocked only for archive construction; this fixture is never published.
        with tempfile.TemporaryDirectory() as d:
            root=Path(d);source=root/'source';models=root/'models';models.mkdir()
            (models/'manifest.json').write_text('{}')
            (source/'runtime/python').mkdir(parents=True)
            (source/'runtime/python/python.exe').write_bytes(b'private interpreter fixture')
            (source/'runtime/site-packages').mkdir()
            (source/'runtime/site-packages/dependency.py').write_text('fixture = True')
            args=['prepare-editing-runtime.py','--provider',provider,'--source',str(source),'--python','runtime/python/python.exe',
                  '--models',str(models),'--tts','tts-codec_llm-0.6b-amh-int8_onnx','--output',str(root/'output')]
            with patch('sys.argv',args),patch.object(runtime.subprocess,'run',return_value=SimpleNamespace(returncode=0,stdout='{"probe":true,"synthesis_checked":true,"alignment_checked":true,"edit_checked":true}\n')),contextlib.redirect_stdout(io.StringIO()):
                runtime.main()
            receipt=json.loads((root/'output/runtime-preparation.json').read_text())
            archive=root/'output'/receipt['artifact']['filename']
            self.assertFalse(receipt['publication_ready'])
            self.assertTrue(receipt['probe']['synthesis_checked'])
            self.assertEqual(hashlib.sha256(archive.read_bytes()).hexdigest(),receipt['artifact']['sha256'])
            with tarfile.open(archive) as tar:
                entries=tar.getmembers();self.assertEqual(entries[0].name,'pack.json')
                pack=json.load(tar.extractfile(entries[0]));declared={f['path']:f for f in pack['files']}
                self.assertEqual(sum(e.size for e in entries),receipt['artifact']['unpacked_size'])
                self.assertTrue(declared[pack['python']]['executable'])
                descriptor='runtime/tts-installation.json' if provider=='small-tts' else 'runtime/voice-edit-installation.json'
                self.assertIn(descriptor,declared)
                config=json.load(tar.extractfile(descriptor));self.assertEqual(config['capabilities'],['tts-flow_lm-90m-amh-fp32' if provider=='small-tts' else 'edit-codec_llm-0.6b-amh'])
                self.assertEqual(set(declared),{e.name for e in entries[1:]})
                for entry in entries[1:]:
                    self.assertTrue(entry.name.startswith('runtime/'))
                    self.assertTrue(entry.isfile())
                    self.assertEqual(hashlib.sha256(tar.extractfile(entry).read()).hexdigest(),declared[entry.name]['sha256'])


if __name__=='__main__':unittest.main()
