import copy
import hashlib
import importlib.util
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest

spec = importlib.util.spec_from_file_location(
    "publish_studio", Path(__file__).resolve().parents[1] / "publish-studio-models.py")
publisher = importlib.util.module_from_spec(spec)
spec.loader.exec_module(publisher)


class StudioPublication(unittest.TestCase):
    def test_uploads_default_to_private_and_public_requires_reviewed_staging(self):
        self.assertEqual(publisher.publication_target(), (publisher.STAGING_REPOSITORY, 'development', True))
        self.assertEqual(publisher.publication_target(True, 'a'*40), (publisher.REPOSITORY, None, False))
        for revision in (None, 'main', 'development', 'a'*39):
            with self.subTest(revision=revision), self.assertRaises(ValueError):
                publisher.publication_target(True, revision)
        with self.assertRaises(ValueError):
            publisher.publication_target(False, 'a'*40)
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        folder = self.root / "test-model"
        folder.mkdir()
        self.data = b"model fixture"
        (folder / "weights.bin").write_bytes(self.data)
        (folder / "private-training-notes.txt").write_text("must not be published")
        self.manifest = {"manifest_version": 1, "bundles": {"cpu-low": {"asr": "test-model"}},
                         "variants": [{"id": "test-model", "folder": "test-model", "requires": [],
                                       "total_bytes": len(self.data), "files": [{"path": "weights.bin",
                                       "bytes": len(self.data), "sha256": hashlib.sha256(self.data).hexdigest()}]}]}

    def test_only_explicit_files_enter_publication(self):
        self.assertEqual(list(publisher.model_files(self.root, self.manifest)), ["test-model/weights.bin"])

    def test_publication_blocks_internal_metadata_without_echoing_it(self):
        examples = [
            ('/home/' + 'release-owner/models/', 'private filesystem path'),
            ('C:\\Users\\release-owner\\models', 'private filesystem path'),
            ('C:\\Users\\release owner\\models', 'private filesystem path'),
            ('/Users/' + 'release owner/models/', 'private filesystem path'),
            ('10.' + '1.2.3', 'private network address'),
            ('_work_' + 'codec/result.json', 'internal experiment reference'),
            ('research/' + 'low_end_inference/export.onnx', 'internal experiment reference'),
            ('not verified offline', 'unfinished publication note'),
            ('hf_' + 'a' * 32, 'credential marker'),
        ]
        for text, reason in examples:
            manifest = copy.deepcopy(self.manifest)
            manifest['variants'][0]['notes'] = text
            with self.subTest(reason=reason), self.assertRaisesRegex(ValueError, reason) as error:
                publisher.validate_public_release(manifest, {})
            self.assertNotIn(text, str(error.exception))

    def test_publication_scans_allowlisted_text_and_leaves_source_unchanged(self):
        path = self.root / 'test-model/README.md'
        text = 'Use ' + '/home/' + 'release-owner/models/ for inference.'
        path.write_text(text)
        selected = {'test-model/README.md': {'source': path, 'bytes': path.stat().st_size}}
        with self.assertRaisesRegex(ValueError, 'private filesystem path'):
            publisher.validate_public_release(self.manifest, selected)
        self.assertEqual(path.read_text(), text)
        # An unselected development file must not enter or block publication.
        publisher.validate_public_release(self.manifest, {})

    def test_publication_preserves_attribution_api_examples_and_real_limits(self):
        text = ('Copyright EthiopicAI. https://github.com/facebookresearch/omnilingual-asr. '
                'Use <models>/codec-higgs_audio_v2-fp32. Always use FP32. '
                'CPU latency is estimated, not measured on x86. See LICENSES/Meta-Llama-3.txt.')
        publisher.validate_public_text(text, 'README.md')

    def test_extensionless_notices_are_scanned_before_public_promotion(self):
        path = self.root / 'NOTICE'
        path.write_text('hf_' + 'a' * 32)
        with self.assertRaisesRegex(ValueError, 'credential marker'):
            publisher.validate_public_release(self.manifest, {'NOTICE': {'source': path, 'bytes': path.stat().st_size}})

    def test_corrupt_file_stops_before_any_upload(self):
        (self.root / "test-model/weights.bin").write_bytes(b"other fixture")
        with self.assertRaisesRegex(ValueError, "does not match"):
            publisher.model_files(self.root, self.manifest)

    def test_traversal_and_absolute_paths_are_rejected(self):
        for path in ("../secret", "/etc/passwd", "a/../secret", "a\\secret", "a//secret", "./secret"):
            with self.subTest(path=path), self.assertRaises(ValueError):
                publisher.safe_path(path)

    def test_missing_dependency_or_bundle_cannot_be_advertised(self):
        for kind in ("dependency", "bundle"):
            manifest = copy.deepcopy(self.manifest)
            if kind == "dependency":
                manifest["variants"][0]["requires"] = ["missing-codec"]
            else:
                manifest["bundles"]["cpu-low"]["tts"] = "missing-tts"
            with self.subTest(kind=kind), self.assertRaises(ValueError):
                publisher.model_files(self.root, manifest)

    def test_hosting_fields_preserve_owner_data_and_pin_urls(self):
        original = copy.deepcopy(self.manifest)
        hosted = publisher.hosted_manifest(self.manifest, "a" * 40)
        self.assertEqual(self.manifest, original)
        self.assertEqual(hosted["variants"], original["variants"])
        self.assertEqual(publisher.file_url(hosted, "test-model", "weights.bin"),
                         "https://huggingface.co/ethiopic/ethiopic-studio-models/resolve/" + "a" * 40
                         + "/test-model/weights.bin")
        with self.assertRaises(ValueError):
            publisher.hosted_manifest(self.manifest, "main")

    def test_hub_content_verification_covers_git_and_lfs(self):
        expected = publisher.model_files(self.root, self.manifest)
        blob = hashlib.sha1(f"blob {len(self.data)}\0".encode() + self.data).hexdigest()
        remote = SimpleNamespace(rfilename="test-model/weights.bin", size=len(self.data),
                                 lfs=None, blob_id=blob)
        info = SimpleNamespace(siblings=[remote])
        publisher.verify_tree(info, expected)
        remote.lfs = SimpleNamespace(sha256=hashlib.sha256(self.data).hexdigest())
        publisher.verify_tree(info, expected)
        remote.lfs.sha256 = "0" * 64
        with self.assertRaisesRegex(ValueError, "checksum mismatch"):
            publisher.verify_tree(info, expected)


if __name__ == "__main__":
    unittest.main()
