import copy
import importlib.util
import json
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('release_catalog', ROOT/'scripts/build-release-catalog.py')
catalog = importlib.util.module_from_spec(spec)
spec.loader.exec_module(catalog)


class ReleaseCatalogTests(unittest.TestCase):
    def test_bundled_migration_resources_do_not_reintroduce_private_build_locations(self):
        import re
        patterns = [r'/(?:home|Users)/[^/\s]+/', r'[A-Z]:[\\/]+Users[\\/]+',
                    '_work' + '_', 'asr_' + 'finetune', 'OmniVoice/' + 'exp/']
        for file in (catalog.RESOURCES / 'naming-migration').iterdir():
            if file.is_file():
                text = file.read_text(encoding='utf-8')
                for pattern in patterns:
                    self.assertIsNone(re.search(pattern, text), file.name)

    def test_master_inventory_contains_all_sources_and_exact_pinned_downloads(self):
        built = catalog.build()
        self.assertEqual(json.loads((catalog.RESOURCES/'release-catalog.json').read_text(encoding='utf-8')), built)
        self.assertGreaterEqual(len(built['artifacts']),96)
        self.assertEqual({r['os'] for r in built['catalogs']['speech_runtimes']['releases']},{'macos','windows'})
        self.assertEqual({r['profile'] for r in built['catalogs']['recognition_packages']['releases'] if r['os']=='windows'},
                         {'cpu-low','cpu-mid','cpu-high','gpu-low','gpu-pascal','gpu-modern'})
        self.assertEqual(len(built['inventory']),15)
        for key, name in catalog.SOURCES.items():
            self.assertEqual(built['catalogs'][key],json.loads((catalog.RESOURCES/name).read_text(encoding='utf-8')))
        for variant in built['catalogs']['models']['variants']:
            files=[v for v in built['artifacts'].values() if v['component']==variant['id']]
            self.assertEqual(sum(v['bytes'] for v in files),variant['total_bytes'])
        self.assertFalse(built['update_policy']['automatic_checks_enabled'])
        self.assertFalse(built['update_policy']['automatic_downloads_enabled'])

    def test_model_dependencies_and_tiers_cannot_point_at_missing_or_cyclic_models(self):
        good=catalog.build()
        for case in ['missing','cycle','tier']:
            data=copy.deepcopy(good);variants=data['catalogs']['models']['variants']
            if case=='missing':variants[0]['requires']=['missing-model']
            elif case=='cycle':variants[0]['requires']=[variants[1]['id']];variants[1]['requires']=[variants[0]['id']]
            else:next(iter(data['catalogs']['models']['bundles'].values()))['asr']='missing-model'
            with self.assertRaises(ValueError):catalog.validate(data)

    def test_checksums_sizes_and_derived_index_cannot_drift(self):
        good=catalog.build()
        for field,value in [('sha256','bad'),('bytes',-1),('bytes',True),('url','https://evil.example/model')]:
            bad=copy.deepcopy(next(iter(good['artifacts'].values())));bad[field]=value
            with self.assertRaises(ValueError):catalog.validate_artifact(bad)
        data=copy.deepcopy(good);next(iter(data['artifacts'].values()))['bytes']+=1
        with self.assertRaisesRegex(ValueError,'index drifted'):catalog.validate(data)
        data=copy.deepcopy(good);data['catalogs']['models']['variants'][0]['total_bytes']+=1
        with self.assertRaisesRegex(ValueError,'Model size'):catalog.validate(data)

    def test_discovery_and_artifacts_reject_credentials_mutable_refs_and_traversal(self):
        artifact=copy.deepcopy(next(iter(catalog.build()['artifacts'].values())))
        for url in ['http://huggingface.co/ethiopic/model',
                    'https://huggingface.co/ethiopic/ethiopic-studio-models/resolve/main/file',
                    'https://user:secret@files.pythonhosted.org/packages/a.whl',
                    'https://files.pythonhosted.org/packages/a.whl?token=secret',
                    'https://files.pythonhosted.org/packages/%2e%2e/a.whl',
                    'https://files.pythonhosted.org:444/packages/a.whl']:
            with self.assertRaises(ValueError):catalog.validate_artifact({**artifact,'url':url})
        for path in ['../outside','a/../outside','/absolute','a\\b','.','a//b']:
            self.assertFalse(catalog.safe_path(path))
        data=catalog.build();data['update_policy']['catalog_endpoint']='https://evil.example/catalog.json'
        with self.assertRaisesRegex(ValueError,'first-party'):catalog.validate(data)

    def test_archive_index_supports_actual_recognition_and_tts_runtime_formats(self):
        data=catalog.build();artifact={'kind':'runtime','url':'https://huggingface.co/ethiopic/studio-runtimes/resolve/'+'a'*40+'/runtime.tar.gz','size':10,'unpacked_size':20,'sha256':'b'*64}
        data['catalogs']['recognition_packages']['releases']=[{'os':'macos','arch':'aarch64','packs':[artifact]}]
        data['catalogs']['speech_runtimes']['releases']=[{'os':'macos','arch':'aarch64','artifact':artifact}]
        data['artifacts']=catalog.artifacts_for(data['catalogs']);catalog.validate(data)
        self.assertEqual([v['path'] for v in data['artifacts'].values() if v['component']=='recognition_packages'],['runtime.tar.gz'])
        self.assertEqual([v['path'] for v in data['artifacts'].values() if v['component']=='speech_runtimes'],['runtime.tar.gz'])

    def test_unpublished_components_and_missing_trust_are_explicit_release_blockers(self):
        data=catalog.build();errors=catalog.release_blockers(data)
        self.assertTrue(any('local draft' in error for error in errors))
        self.assertTrue(any('public key' in error for error in errors))
        self.assertTrue(any('Tauri updater' in error for error in errors))
        self.assertTrue(any('Voice Cleanup' in error for error in errors))
        self.assertFalse(any('recognition_packages' in error for error in errors))
        self.assertFalse(any('recognition_packages (windows-x86_64)' in error for error in errors))
        self.assertFalse(any('recognition_packages (macos-aarch64)' in error for error in errors))
        data['catalogs']['recognition_packages']['releases']=[]
        self.assertTrue(any('recognition_packages' in error for error in catalog.release_blockers(data)))
        self.assertEqual(data['supported_app_platforms'],['macos-aarch64','windows-x86_64'])


if __name__ == '__main__':unittest.main()
