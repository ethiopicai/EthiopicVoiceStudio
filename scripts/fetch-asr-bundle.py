#!/usr/bin/env python3
"""Developer release-source sync. Never shipped to end users; no training files copied."""
import argparse
import hashlib
import json
from pathlib import Path
import shlex
import subprocess


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--host', required=True, help='SSH host containing your released ASR bundle')
    parser.add_argument('--latest', required=True, help='Absolute path to your latest-release symlink')
    parser.add_argument('--destination', type=Path, default=Path('.local-asr/bundle'))
    args = parser.parse_args()
    if args.host.startswith('-'):
        parser.error('Invalid SSH host')
    resolved = subprocess.check_output(['ssh', '-o', 'BatchMode=yes', args.host,
                                       'readlink -f ' + shlex.quote(args.latest)], text=True).strip()
    if not resolved.startswith(str(Path(args.latest).parent) + '/amharic-ctc-v'):
        raise RuntimeError('Latest does not resolve to an immutable released bundle')
    files = ['README.md', 'STREAMING.md', 'requirements.txt', 'model/model.safetensors', 'model/config.json',
             'model/tokenizer.model', 'model/version.json', 'omniasr_hf/__init__.py',
             'omniasr_hf/audio_text.py', 'omniasr_hf/transcribe.py']
    remote = 'cd ' + shlex.quote(resolved) + ' && sha256sum ' + ' '.join(map(shlex.quote, files))
    checksums = subprocess.check_output(['ssh', '-o', 'BatchMode=yes', args.host, remote], text=True)
    args.destination.mkdir(parents=True, exist_ok=True)
    # Explicit file allowlist keeps this independent of changes in the training repository.
    for name in files:
        target = args.destination / name
        target.parent.mkdir(parents=True, exist_ok=True)
        subprocess.run(['rsync', '-a', '--partial', '-e', 'ssh -o BatchMode=yes',
                        f'{args.host}:{resolved}/{name}', str(target)], check=True)
    for line in checksums.splitlines():
        expected, name = line.split(maxsplit=1)
        with (args.destination / name).open('rb') as file:
            if hashlib.file_digest(file, 'sha256').hexdigest() != expected:
                raise RuntimeError(f'Release checksum mismatch: {name}')
    version = json.loads((args.destination / 'model/version.json').read_text())['model_version']
    (args.destination.parent / 'source-release.json').write_text(json.dumps(
        {'source': resolved, 'model_version': version, 'checksums': checksums}, indent=2))
    print(f'Verified released ASR {version}. Latest was resolved once before transfer.')


if __name__ == '__main__':
    main()
