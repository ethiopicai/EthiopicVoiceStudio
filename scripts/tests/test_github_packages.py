import hashlib
import importlib.util
import json
from pathlib import Path
import struct
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('github_packages', Path(__file__).resolve().parents[1] / 'prepare-github-packages.py')
packages = importlib.util.module_from_spec(spec)
spec.loader.exec_module(packages)


class GitHubPackages(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.assets = self.root / 'assets'
        config = self.root / 'apps/desktop/src-tauri/tauri.conf.json'
        config.parent.mkdir(parents=True)
        config.write_text(json.dumps({'version': '0.1.0-rc.2', 'productName': 'EthiopicAI Voice Studio'}))
        self.commit = 'a' * 40
        self.version = '0.1.0-rc.2'

    def fixture(self, target):
        release = self.root / 'target' / target / 'release'
        system, arch, support, _, _ = packages.identity(target, self.version)
        if system == 'macos':
            binary = release / 'bundle/macos/EthiopicAI Voice Studio.app/Contents/MacOS/ethiopic-voice-studio'
            binary.parent.mkdir(parents=True)
            binary.write_bytes(b'\xcf\xfa\xed\xfe' + struct.pack('<I', 0x0100000C if arch == 'aarch64' else 0x01000007) + b'\0' * 56)
            directory = self.root / '.release' / f'macos-{self.version}{"-intel" if arch == "x86_64" else ""}-candidate'
            directory.mkdir(parents=True)
            name = f'EthiopicAI-Voice-Studio-{self.version}-macos-{"arm64" if arch == "aarch64" else "x64"}-candidate.dmg'
            installer = directory / name
            installer.write_bytes(b'installer fixture ' + target.encode())
            (directory / 'build-status.json').write_text(json.dumps({'version': self.version, 'arch': arch, 'target': target,
                'public_release': False, 'sha256': packages.digest(installer)}))
        else:
            binary = release / 'ethiopic-voice-studio.exe'
            binary.parent.mkdir(parents=True)
            header = bytearray(64)
            header[:2] = b'MZ'
            struct.pack_into('<I', header, 60, 64)
            binary.write_bytes(header + b'PE\0\0' + struct.pack('<H', 0x8664))
            installer = release / 'bundle/nsis/EthiopicAI Voice Studio_0.1.0-rc.2_x64-setup.exe'
            installer.parent.mkdir(parents=True)
            installer.write_bytes(b'installer fixture ' + target.encode())
        return binary, installer

    def all_packages(self):
        for target in packages.TARGETS:
            self.fixture(target)
            packages.stage(target, self.commit, self.assets, self.root)

    def test_three_architectures_produce_unique_checked_assets_and_honest_draft_notes(self):
        self.all_packages()
        output = self.root / 'draft'
        self.assertEqual(packages.draft(self.assets, output, self.commit), 'v0.1.0-rc.2')
        self.assertEqual(len(list(self.assets.iterdir())), 9)
        notes = (output / 'RELEASE_NOTES.md').read_text()
        self.assertIn('Interface testing only', notes)
        self.assertIn('not notarized', notes)
        files = (output / 'release-files.txt').read_text().splitlines()
        self.assertEqual(len(files), 9)
        self.assertTrue(all(Path(name).is_file() for name in files))

    def test_wrong_executable_architecture_is_rejected_before_packaging(self):
        for target in packages.TARGETS:
            with self.subTest(target=target):
                binary, _ = self.fixture(target)
                binary.write_bytes(b'not the requested architecture')
                with self.assertRaises(ValueError):
                    packages.stage(target, self.commit, self.assets, self.root)
        self.assertFalse(self.assets.exists())

    def test_tampered_installer_and_checksum_are_rejected(self):
        self.all_packages()
        installer = next(self.assets.glob('*.exe'))
        original = installer.read_bytes()
        installer.write_bytes(b'changed installer')
        with self.assertRaisesRegex(ValueError, 'checksum'):
            packages.draft(self.assets, self.root / 'draft', self.commit)
        installer.write_bytes(original)
        Path(str(installer) + '.sha256').write_text('wrong checksum')
        with self.assertRaisesRegex(ValueError, 'checksum'):
            packages.draft(self.assets, self.root / 'draft', self.commit)

    def test_mixed_commits_and_wrong_release_tags_are_rejected(self):
        self.all_packages()
        with self.assertRaisesRegex(ValueError, 'metadata'):
            packages.draft(self.assets, self.root / 'draft', 'b' * 40)
        with self.assertRaisesRegex(ValueError, 'tag'):
            packages.draft(self.assets, self.root / 'draft', self.commit, 'v9.9.9')

    def test_extra_files_cannot_be_attached_to_a_release(self):
        self.all_packages()
        (self.assets / '.env').write_text('private fixture; never attach')
        with self.assertRaisesRegex(ValueError, 'Unexpected files') as error:
            packages.draft(self.assets, self.root / 'draft', self.commit)
        self.assertNotIn('private fixture', str(error.exception))

    def test_installer_metadata_cannot_escape_the_asset_directory(self):
        self.all_packages()
        file = next(self.assets.glob('*.json'))
        record = json.loads(file.read_text())
        record['installer'] = '../outside.exe'
        file.write_text(json.dumps(record))
        with self.assertRaisesRegex(ValueError, 'metadata'):
            packages.draft(self.assets, self.root / 'draft', self.commit)

    def test_missing_platform_cannot_make_a_partial_three_platform_release(self):
        self.all_packages()
        next(self.assets.glob('*.json')).unlink()
        with self.assertRaisesRegex(ValueError, 'exactly one package'):
            packages.draft(self.assets, self.root / 'draft', self.commit)

    def test_existing_staged_files_are_never_silently_replaced(self):
        target = 'x86_64-pc-windows-msvc'
        self.fixture(target)
        packages.stage(target, self.commit, self.assets, self.root)
        with self.assertRaisesRegex(ValueError, 'Refusing to replace'):
            packages.stage(target, self.commit, self.assets, self.root)

    def test_oversized_asset_is_rejected_before_hashing_the_payload(self):
        self.all_packages()
        with patch.object(packages, 'MAX_ASSET_BYTES', 1), patch.object(packages, 'digest', side_effect=AssertionError('Must reject before reading a large file')):
            with self.assertRaisesRegex(ValueError, 'Invalid release installer'):
                packages.draft(self.assets, self.root / 'draft', self.commit)


if __name__ == '__main__':
    unittest.main()
