#!/usr/bin/env python3
"""Maintainer-only activation of numerically validated, offline cleanup resources."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import wave


def digest(path):
    with path.open('rb') as file:
        return hashlib.file_digest(file, 'sha256').hexdigest()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--source', type=Path, default=Path('.local-cleanup'))
    parser.add_argument('--destination', type=Path, required=True)
    args = parser.parse_args()
    if platform.system() != 'Darwin' or platform.machine() != 'arm64':
        raise SystemExit('This cleanup runtime has only been validated on macOS arm64.')
    source, destination = args.source.resolve(), args.destination.resolve()
    repo = Path(__file__).resolve().parent.parent
    report = json.loads((source / 'validation-reference.json').read_text())
    if not report.get('passed') or not report.get('fullModel', {}).get('passed', False):
        # Older reports store the aggregate decision at the top level only.
        if not report.get('passed') or not report.get('fullModel', {}).get('waveform', {}).get('passed'):
            raise RuntimeError('A passing full-model reference comparison is required')
    for path in [source / 'source/model.safetensors', source / 'source/config.json',
                 *sorted((source / 'source/models').glob('*.py')), repo / 'workers/cleanup/mamba_reference.py']:
        key = next((key for key in report['provenance'] if key.endswith(path.relative_to(source).as_posix() if path.is_relative_to(source) else 'workers/cleanup/mamba_reference.py')), None)
        if key is None or report['provenance'][key] != digest(path):
            raise RuntimeError('Validated source changed; repeat the reference comparison before installing')
    destination.mkdir(parents=True, exist_ok=True)
    import fcntl
    lock = (destination.parent / 'recognition.lock').open('a+b')
    fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    active, previous, stage = destination / 'active', destination / 'previous', destination / 'staging'
    if not active.exists() and previous.exists(): previous.rename(active)
    if stage.exists(): shutil.rmtree(stage)
    stage.mkdir()
    try:
        # Only inference artifacts are distributed. No source samples/reference oracle.
        bundle = stage / 'bundle'
        for name in ['LICENSE', 'README.md', 'config.json', 'model.safetensors', 'version.json',
                     'models/codec_module_time_d4.py', 'models/generator_SEMamba_time_d4.py',
                     'models/mamba_block2_SEMamba.py', 'models/stfts.py']:
            target = bundle / name
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(source / 'source' / name, target)
        shutil.copytree(source / 'runtime', stage / 'runtime', symlinks=True,
                        ignore=shutil.ignore_patterns('__pycache__', '*.pyc'))
        shutil.copytree(repo / 'workers/cleanup/licenses', stage / 'licenses')
        (stage / 'provider').mkdir()
        for name in ['worker.py', 'mamba_reference.py']:
            shutil.copy2(repo / 'workers/cleanup' / name, stage / 'provider' / name)
        shutil.copy2(source / 'validation-reference.json', stage / 'validation-reference.json')
        python = next((stage / 'runtime/python').glob('*/bin/python3'))
        site = stage / 'runtime/site-packages'
        sample, output, metadata = stage / 'check.wav', stage / 'check-cleaned.wav', stage / 'check.json'
        with wave.open(str(sample), 'wb') as file:
            file.setparams((1, 2, 16000, 0, 'NONE', 'not compressed'))
            file.writeframes(b'\0\0' * 1600)
        print('Validating private Voice Cleanup runtime and real enhancement…', flush=True)
        subprocess.run([str(python), '-I', '-B', '-u', '-c',
                        "import runpy,sys,os;sys.path.insert(0,sys.argv.pop(1));worker=sys.argv.pop(1);sys.path.insert(0,os.path.dirname(worker));runpy.run_path(worker,run_name='__main__')",
                        str(site), str(stage / 'provider/worker.py'), '--bundle', str(bundle),
                        '--audio', str(sample), '--output', str(output), '--metadata', str(metadata)],
                       check=True, timeout=180, stdin=subprocess.DEVNULL,
                       env={**os.environ, 'ETHIOPIC_MANAGED_WORKER': '0'})
        with wave.open(str(output)) as file:
            if (file.getframerate(), file.getnchannels(), file.getnframes()) != (16000, 1, 1600):
                raise RuntimeError('Enhancement output dimensions changed')
        version = json.loads((bundle / 'version.json').read_text())['model_version']
        if json.loads(metadata.read_text())['model_version'] != version:
            raise RuntimeError('Model version mismatch')
        for path in [sample, output, metadata]: path.unlink()
        files = [{'path': path.relative_to(stage).as_posix(), 'sha256': digest(path)}
                 for path in sorted(stage.rglob('*')) if path.is_file()]
        config = {'schema_version': 1, 'os': 'macos', 'arch': 'aarch64', 'python': python.relative_to(stage).as_posix(),
                  'site_packages': site.relative_to(stage).as_posix(), 'model_version': version,
                  'backend': 'torch-cpu-reference', 'validated': True,
                  'adapter_sha256': digest(repo / 'workers/cleanup/mamba_reference.py'), 'files': files}
        (stage / 'installation.json').write_text(json.dumps(config, indent=2))
        if previous.exists(): shutil.rmtree(previous)
        if active.exists(): active.rename(previous)
        try: stage.rename(active)
        except Exception:
            if previous.exists(): previous.rename(active)
            raise
        print('Voice Cleanup activated. Working ASR resources were not modified.')
    except Exception:
        if stage.exists(): shutil.rmtree(stage)
        raise


if __name__ == '__main__':
    main()
