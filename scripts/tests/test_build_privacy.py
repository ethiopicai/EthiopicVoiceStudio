import importlib.util
from pathlib import Path
import tempfile
import unittest


spec = importlib.util.spec_from_file_location('build_privacy', Path(__file__).resolve().parents[1] / 'build-privacy.py')
privacy = importlib.util.module_from_spec(spec)
spec.loader.exec_module(privacy)


class BuildPrivacy(unittest.TestCase):
    def test_remapping_preserves_encoded_flags_and_paths_with_spaces(self):
        with tempfile.TemporaryDirectory(prefix='build privacy ') as directory:
            home = Path(directory) / 'person'
            root = home / 'source project'
            cargo = home / 'cargo cache'
            original = {'HOME': str(home), 'CARGO_HOME': str(cargo),
                        'CARGO_ENCODED_RUSTFLAGS': '-C\x1fdebuginfo=0'}
            result = privacy.build_environment(root, original)
            flags = result['CARGO_ENCODED_RUSTFLAGS'].split('\x1f')
            self.assertEqual(flags[:2], ['-C', 'debuginfo=0'])
            self.assertIn(f'--remap-path-prefix={root}=/studio/src', flags)
            self.assertIn(f'--remap-path-prefix={root.resolve()}=/studio/src', flags)
            self.assertIn(f'--remap-path-prefix={cargo.resolve()}=/rust/cargo', flags)
            self.assertEqual(original['CARGO_ENCODED_RUSTFLAGS'], '-C\x1fdebuginfo=0')

    def test_real_build_paths_are_rejected_in_both_executable_encodings(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / 'source'
            home = Path(directory) / 'person'
            binary = Path(directory) / 'binary'
            env = {'HOME': str(home)}
            for path in [root, home]:
                for encoding in ['utf-8', 'utf-16-le']:
                    for value in [str(path), str(path).replace('/', '\\')]:
                        with self.subTest(encoding=encoding, separators='\\' in value):
                            binary.write_bytes(b'header' + value.encode(encoding) + b'/private')
                            with self.assertRaisesRegex(ValueError, 'build-machine path') as error:
                                privacy.reject_build_paths(binary, root, env)
                            self.assertNotIn(directory, str(error.exception))
            binary.write_bytes(b'header /studio/src/app.rs /rust/cargo/library /build/user/compiler')
            privacy.reject_build_paths(binary, root, env)


if __name__ == '__main__':
    unittest.main()
