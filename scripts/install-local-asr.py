#!/usr/bin/env python3
"""Developer-only offline activation of a prepared first-party bundle/private runtime.

No dependency installation or downloads occur here. End users will use resource packs.
"""
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
    parser.add_argument('--source', type=Path, default=Path('.local-asr'))
    parser.add_argument('--destination', type=Path, required=True)
    args = parser.parse_args()
    source = args.source.resolve()
    destination = args.destination.resolve()
    destination.mkdir(parents=True, exist_ok=True)
    # Coordinate with the native supervisor before replacing a live runtime.
    lock = (destination.parent / 'recognition.lock').open('a+b')
    if os.name == 'posix':
        import fcntl
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    else:
        import msvcrt
        lock.seek(0)
        lock.write(b'0')
        lock.flush()
        lock.seek(0)
        msvcrt.locking(lock.fileno(), msvcrt.LK_NBLCK, 1)
    if not (destination / 'active').exists() and (destination / 'previous').exists():
        (destination / 'previous').rename(destination / 'active')
    stage = destination / 'staging'
    if stage.exists():
        shutil.rmtree(stage)
    stage.mkdir()
    try:
        for name in ['bundle', 'runtime']:
            shutil.copytree(source / name, stage / name, symlinks=True,
                            ignore=shutil.ignore_patterns('__pycache__', '*.pyc'))
        candidates = list((stage / 'runtime/python').glob('*/bin/python3'))
        if len(candidates) != 1:
            raise RuntimeError('Expected one private Python runtime')
        python = candidates[0]
        site = stage / 'runtime/site-packages'
        subprocess.run([str(python), '-I', '-B', '-c',
                        'import sys;sys.path.insert(0,sys.argv[1]);import torch,transformers,sentencepiece,numpy,scipy;'
                        'assert torch.__version__.split("+")[0]=="2.9.1";assert transformers.__version__=="4.55.4"', str(site)], check=True)
        # The released bundle must support both offline and streaming entry points.
        subprocess.run([str(python), '-I', '-B', '-c',
                        'import sys;sys.path[:0]=sys.argv[1:3];'
                        'from omniasr_hf.transcribe import CTCTranscriber,StreamingTranscriber;'
                        'assert callable(CTCTranscriber.decode);assert callable(StreamingTranscriber.feed);'
                        'assert callable(StreamingTranscriber.finish)', str(site), str(stage / 'bundle')],
                       check=True, timeout=60,
                       env={**os.environ, 'HF_HUB_OFFLINE':'1', 'TRANSFORMERS_OFFLINE':'1'})
        version = json.loads((stage / 'bundle/model/version.json').read_text())['model_version']
        # A clean local cold-load/inference check catches runtime/model incompatibility
        # before activation. No microphone, user text, or network is involved.
        print('Checking private runtime and model loading before activation…', flush=True)
        sample, result = stage / 'setup-check.wav', stage / 'setup-check.json'
        with wave.open(str(sample), 'wb') as wav:
            wav.setparams((1, 2, 16000, 0, 'NONE', 'not compressed'))
            wav.writeframes(b'\0\0' * 4000)
        subprocess.run([str(python), '-I', '-B', '-u', '-c',
                        "import runpy,sys;sys.path.insert(0,sys.argv.pop(1));worker=sys.argv.pop(1);runpy.run_path(worker,run_name='__main__')",
                        str(site), str(Path(__file__).resolve().parent.parent / 'workers/asr/worker.py'),
                        '--bundle', str(stage / 'bundle'), '--audio', str(sample), '--output', str(result)],
                       check=True, timeout=180, stdin=subprocess.DEVNULL,
                       env={**os.environ, 'ETHIOPIC_MANAGED_WORKER': '0'})
        if json.loads(result.read_text())['model_version'] != version:
            raise RuntimeError('Model self-test version mismatch')
        sample.unlink()
        result.unlink()
        files = []
        for path in sorted(stage.rglob('*')):
            if path.is_file():
                relative = path.relative_to(stage).as_posix()
                actual = digest(path)
                if actual != digest(source / relative):
                    raise RuntimeError('Resource copy verification failed')
                files.append({'path': relative, 'sha256': actual})
        config = {'schema_version': 1, 'os': {'Darwin': 'macos', 'Linux': 'linux', 'Windows': 'windows'}[platform.system()],
                  'arch': {'arm64': 'aarch64', 'AMD64': 'x86_64'}.get(platform.machine(), platform.machine()),
                  'python': python.relative_to(stage).as_posix(), 'site_packages': site.relative_to(stage).as_posix(),
                  'model_version': version, 'files': files, 'development_installation': True}
        (stage / 'installation.json').write_text(json.dumps(config, indent=2))
        active, previous = destination / 'active', destination / 'previous'
        if previous.exists():
            shutil.rmtree(previous)
        if active.exists():
            active.rename(previous)
        try:
            stage.rename(active)
        except Exception:
            if previous.exists(): previous.rename(active)
            raise
        print(f'Activated local Amharic ASR {version}. Previous installation retained for rollback.')
    except Exception:
        if stage.exists(): shutil.rmtree(stage)
        raise


if __name__ == '__main__':
    main()
