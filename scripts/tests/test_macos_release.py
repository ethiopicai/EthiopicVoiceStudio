import importlib.util
import io
import json
from pathlib import Path
import shutil
import tarfile
import tempfile
import unittest
from unittest.mock import patch

SCRIPTS = Path(__file__).resolve().parents[1]


def load(name):
    spec = importlib.util.spec_from_file_location(name, SCRIPTS / f'{name}.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


packager = load('prepare-macos-asr-release')
builder = load('build-macos-release')


class ReleasePacks(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.source = self.root / 'source'
        self.output = self.root / 'output'
        (self.source / 'bundle/model').mkdir(parents=True)
        (self.source / 'bundle/model/version.json').write_text('{"model_version":"v0.2"}')
        (self.source / 'runtime/python/test/bin').mkdir(parents=True)
        python = self.source / 'runtime/python/test/bin/python3.11'
        python.write_bytes(b'private interpreter fixture')
        python.chmod(0o755)
        self.symlinks_available = True
        try:
            python.with_name('python3').symlink_to('python3.11')
        except OSError as error:
            if getattr(error, 'winerror', None) != 1314:
                raise
            self.symlinks_available = False
            shutil.copyfile(python, python.with_name('python3'))
        (self.source / 'runtime/site-packages').mkdir()
        self.output.mkdir()

    def test_pack_is_reproducible_and_materializes_private_python_links(self):
        if not self.symlinks_available:
            self.skipTest('Windows symlink privilege is unavailable; exercised on macOS')
        first = packager.build_pack(self.source, self.output, 'runtime', 'v0.2')
        second = packager.build_pack(self.source, self.output, 'runtime', 'v0.2')
        self.assertEqual(first, second)
        with tarfile.open(self.output / first['filename']) as archive:
            self.assertEqual(archive.getnames()[0], 'pack.json')
            manifest = json.load(archive.extractfile('pack.json'))
            self.assertEqual(manifest['python'], 'runtime/python/test/bin/python3')
            self.assertTrue(all(member.isfile() for member in archive))
            self.assertEqual(archive.extractfile(manifest['python']).read(), b'private interpreter fixture')
            self.assertEqual(first['unpacked_size'], sum(member.size for member in archive))

    def test_pack_never_follows_links_outside_resources(self):
        if not self.symlinks_available:
            self.skipTest('Windows symlink privilege is unavailable; exercised on macOS')
        outside = self.root / 'outside'
        outside.write_bytes(b'sensitive fixture')
        (self.source / 'bundle/leak').symlink_to(outside)
        with self.assertRaisesRegex(ValueError, 'leaves its pack'):
            packager.build_pack(self.source, self.output, 'model', 'v0.2')
        self.assertFalse(list(self.output.iterdir()))

    def test_directory_links_are_rejected_instead_of_silently_omitting_contents(self):
        if not self.symlinks_available:
            self.skipTest('Windows symlink privilege is unavailable; exercised on macOS')
        (self.source / 'runtime/link').symlink_to('site-packages', target_is_directory=True)
        with self.assertRaisesRegex(ValueError, 'Directory symlinks'):
            packager.entries(self.source, 'runtime')

    def test_newer_os_wheels_cannot_hide_behind_an_older_macos_baseline(self):
        wheel = self.source / 'runtime/site-packages/example.dist-info/WHEEL'
        wheel.parent.mkdir()
        wheel.write_text('Tag: cp311-cp311-macosx_14_0_arm64\n')
        packager.validate_runtime_tags(self.source, '14.0')
        with self.assertRaisesRegex(ValueError, 'needs macOS'):
            packager.validate_runtime_tags(self.source, '13.0')
        wheel.write_text('Tag: cp311-cp311-win_amd64\n')
        with self.assertRaisesRegex(ValueError, 'Unsupported platform'):
            packager.validate_runtime_tags(self.source)

    def test_catalog_pins_a_public_ethiopic_commit_and_keeps_pack_checksums(self):
        model = packager.build_pack(self.source, self.output, 'model', 'v0.2')
        runtime = packager.build_pack(self.source, self.output, 'runtime', 'v0.2')
        catalog = packager.make_catalog([model, runtime], 'v0.2', 'ethiopic/studio', 'a' * 40)
        release = catalog['releases'][0]
        self.assertEqual(release['os'], 'macos')
        self.assertEqual(release['arch'], 'aarch64')
        self.assertEqual(release['packs'][0]['sha256'], model['sha256'])
        self.assertIn('/resolve/' + 'a' * 40 + '/', release['packs'][0]['url'])
        for repo, revision in [('other/studio', 'a' * 40), ('ethiopic/studio', 'main'), ('ethiopic/studio?token=x', 'a' * 40)]:
            with self.assertRaises(ValueError):
                packager.make_catalog([model, runtime], 'v0.2', repo, revision)

    def test_public_build_rejects_empty_catalog_but_local_candidate_is_allowed(self):
        config = self.root / 'apps/desktop/resources'
        config.mkdir(parents=True)
        (config / 'asr-catalog.json').write_text('{"schema_version":1,"releases":[]}')
        with patch.object(builder, 'ROOT', self.root), patch.object(builder.platform, 'system', return_value='Darwin'), patch.object(builder.platform, 'machine', return_value='arm64'):
            with self.assertRaisesRegex(ValueError, 'publish and pin'):
                builder.preflight(False)
            (config / 'asr-catalog.json').write_text(json.dumps({'schema_version': 1, 'releases': [
                {'os': 'windows', 'arch': 'x86_64', 'minimum_windows': '10.0.19041'}]}))
            with self.assertRaisesRegex(ValueError, 'publish and pin'):
                builder.preflight(False)
            builder.preflight(True)

    def test_wrong_build_platform_is_rejected(self):
        with patch.object(builder.platform, 'system', return_value='Windows'):
            with self.assertRaisesRegex(ValueError, 'Apple Silicon or Intel Mac'):
                builder.preflight(True)

    def test_intel_candidate_is_distinct_and_cannot_bypass_public_runtime_checks(self):
        config = self.root / 'apps/desktop/resources'
        config.mkdir(parents=True)
        (config / 'asr-catalog.json').write_text(json.dumps({'schema_version': 1, 'releases': [
            {'os': 'macos', 'arch': 'aarch64', 'minimum_macos': '14.0'}]}))
        with patch.object(builder, 'ROOT', self.root), patch.object(builder.platform, 'system', return_value='Darwin'), patch.object(builder.platform, 'machine', return_value='x86_64'):
            target, details = builder.preflight(True)
            self.assertEqual(target, 'x86_64-apple-darwin')
            self.assertEqual(details['arch'], 'x86_64')
            with self.assertRaisesRegex(ValueError, 'publish and pin'):
                builder.preflight(False)

    def test_candidate_can_cross_compile_but_acceptance_is_architecture_specific(self):
        with patch.object(builder.platform, 'system', return_value='Darwin'), patch.object(builder.platform, 'machine', return_value='arm64'):
            target, details = builder.preflight(True, target='x86_64-apple-darwin')
            self.assertEqual(details['cpu'], 'x86_64')
        report = self.root / 'acceptance-intel.json'
        report.write_text(json.dumps({'schema_version': 1, 'app_version': '0.1.0-rc.1',
            'platform': 'macos', 'arch': 'x86_64', 'checks': {name: {'passed': True, 'evidence': 'fixture'} for name in builder.ACCEPTANCE_CHECKS}}))
        builder.check_acceptance(report, '0.1.0-rc.1', 'x86_64')
        with self.assertRaisesRegex(ValueError, 'does not match'):
            builder.check_acceptance(report, '0.1.0-rc.1')

    def test_acceptance_cannot_reuse_another_version_or_hide_pending_checks(self):
        report = self.root / 'acceptance.json'
        data = {'schema_version': 1, 'app_version': '0.1.0-rc.1', 'platform': 'macos', 'arch': 'aarch64',
                'checks': {name: {'passed': True, 'evidence': 'fixture evidence'} for name in builder.ACCEPTANCE_CHECKS}}
        report.write_text(json.dumps(data))
        builder.check_acceptance(report, '0.1.0-rc.1')
        with self.assertRaisesRegex(ValueError, 'does not match'):
            builder.check_acceptance(report, '0.1.0')
        data['checks']['real_microphone']['passed'] = False
        report.write_text(json.dumps(data))
        with self.assertRaisesRegex(ValueError, 'real_microphone'):
            builder.check_acceptance(report, '0.1.0-rc.1')
        with self.assertRaisesRegex(ValueError, 'acceptance-report'):
            builder.check_acceptance(None, '0.1.0-rc.1')


if __name__ == '__main__':
    unittest.main()
