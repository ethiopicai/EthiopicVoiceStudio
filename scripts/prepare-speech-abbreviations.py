"""Compile the pinned upstream AbbreviationExpander into offline UI rules.

No model, Python runtime, network or third-party dependency is needed by the UI.
The generated pattern order is significant: it is the upstream expander's order.
Run with --check in validation; updating upstream is an explicit release action.
"""
import argparse
import json
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'third_party/amharic-text-processor'))
from amharic_text_processor.processors.abbreviations import AbbreviationExpander
from amharic_text_processor.assets.abbreviations import ABBREVIATIONS


def compiled():
    # Local spelling correction; leave the pinned upstream files unchanged.
    overrides = json.loads((ROOT / 'scripts/speech-abbreviation-corrections.json').read_text(encoding='utf-8'))
    expander = AbbreviationExpander(mapping={**ABBREVIATIONS, **overrides})
    # Check that the upstream regexes use only syntax shared with JavaScript.
    # The current dictionary has only Ethiopic letters, slashes and dots.
    for pattern, _ in expander._patterns:
        if any(c.isascii() and c not in '/+.\\' for c in pattern.pattern):
            raise ValueError('Review new upstream regex syntax before compiling')
    return json.dumps({
        'source': 'https://github.com/ethiopicai/Amharic-Text-Processor',
        'revision': '3175feaefb31e7af48118ac4760fa3cdc167c7fe',
        'processor': 'AbbreviationExpander()',
        'patterns': [[pattern.pattern, meaning] for pattern, meaning in expander._patterns],
    }, ensure_ascii=False, indent=2) + '\n'


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    destination = ROOT / 'apps/desktop/ui/src/speech-abbreviations.json'
    expected = compiled()
    if args.check:
        if destination.read_text(encoding='utf-8') != expected:
            raise SystemExit('Speech abbreviation rules need regeneration')
    else:
        destination.write_text(expected, encoding='utf-8')
