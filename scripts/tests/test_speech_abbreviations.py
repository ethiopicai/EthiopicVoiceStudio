"""Cross-language contract against the actual, pinned upstream processor."""
import importlib.util
import hashlib
import json
from pathlib import Path
import subprocess
import unittest

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location('speech_abbreviation_compiler', ROOT / 'scripts/prepare-speech-abbreviations.py')
compiler = importlib.util.module_from_spec(spec)
spec.loader.exec_module(compiler)
from amharic_text_processor.processors.abbreviations import AbbreviationExpander
from amharic_text_processor.assets.abbreviations import ABBREVIATIONS


class SpeechAbbreviations(unittest.TestCase):
    def test_vendored_modules_are_unmodified_upstream_blobs(self):
        files = {
            'amharic_text_processor/base.py': '2b8139f234909f434a45af0d95f49a23a38f0917',
            'amharic_text_processor/processors/abbreviations.py': 'baa2299d98812b1c1c362ce873d8280a346294d3',
            'amharic_text_processor/assets/abbreviations.py': 'b70bb9abd521ece2cc81f23d62bbee71ec5c1a63',
            'pyproject.toml': '25c3f2812383fda1925370a0fe3d6c7c8c9d95e9',
        }
        for path, expected in files.items():
            data = (ROOT / 'third_party/amharic-text-processor' / path).read_bytes()
            self.assertEqual(hashlib.sha1(b'blob ' + str(len(data)).encode() + b'\0' + data).hexdigest(), expected, path)

    def test_generated_rules_match_pinned_source(self):
        self.assertEqual((ROOT / 'apps/desktop/ui/src/speech-abbreviations.json').read_text(encoding='utf-8'), compiler.compiled())

    def test_browser_matches_actual_expander_for_entire_dictionary_and_contexts(self):
        overrides = json.loads((ROOT / 'scripts/speech-abbreviation-corrections.json').read_text(encoding='utf-8'))
        expander = AbbreviationExpander(mapping={**ABBREVIATIONS, **overrides})
        inputs = ['ዶ/ር ኢ/ር 12', '(ዶ/ር)፣ ኢ//ር።', 'ዓ.ም. 2018', 'ኡ/ኡ', '12/03/2026', 'me@example.com']
        for key in ABBREVIATIONS:
            inputs.extend([key, f'ሰላም {key} አበበ።', key.replace('/', '//')])
        script = """
import {expandAbbreviations} from './apps/desktop/ui/src/speech-abbreviations.js';
let input='';for await(const chunk of process.stdin)input+=chunk;
process.stdout.write(JSON.stringify(JSON.parse(input).map(text=>expandAbbreviations(text).text)));
"""
        result = subprocess.run(['node', '--input-type=module', '-e', script], cwd=ROOT,
                                input=json.dumps(inputs), capture_output=True, text=True, encoding='utf-8', check=True)
        self.assertEqual(json.loads(result.stdout), [expander.apply(value)['text'] for value in inputs])


if __name__ == '__main__':
    unittest.main()
