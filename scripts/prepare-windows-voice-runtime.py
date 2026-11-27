#!/usr/bin/env python3
"""Prepare and qualify isolated portable Windows CPU/GPU speech runtime archives.

Builds run on native Windows. A prior, verified ASR interpreter/CRT payload is
read-only input; dependencies and models are prepared in new directories. This
maintainer tool never changes user app data or publishes an archive.
"""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
POCKET_COMMIT = 'ee2ebc89aba9d0121d04d18e29c20eea75bb624e'
POCKET_SOURCE = f'pocket-tts @ git+https://github.com/kyutai-labs/pocket-tts.git@{POCKET_COMMIT}'


def script(name):
    spec = importlib.util.spec_from_file_location('studio_' + name.replace('-', '_'), ROOT / 'scripts' / (name + '.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def private_python(source):
    direct = source / 'runtime/python/python.exe'
    values = [direct] if direct.is_file() else list((source / 'runtime/python').glob('*/python.exe'))
    if len(values) != 1:
        raise ValueError('Expected exactly one private Windows interpreter')
    return values[0]


def copy_interpreter(base, source):
    if source.exists():
        raise ValueError('Use a new isolated source directory')
    base = base.resolve(strict=True)
    if source.resolve().is_relative_to(base):
        raise ValueError('Do not write inside a previously validated runtime')
    original = private_python(base)
    lock = json.loads((ROOT / 'workers/asr/windows-vc-runtime.lock.json').read_text(encoding='utf-8'))
    for entry in lock['files']:
        dll = original.parent / entry['path']
        if not dll.is_file() or dll.is_symlink() or dll.stat().st_size != entry['bytes'] or hashlib.sha256(dll.read_bytes()).hexdigest() != entry['sha256']:
            raise ValueError('Source interpreter is missing its pinned private Windows CRT')
    source.mkdir(parents=True)
    shutil.copytree(base / 'runtime/python', source / 'runtime/python')
    if (base / 'runtime/licenses').is_dir():
        shutil.copytree(base / 'runtime/licenses', source / 'runtime/licenses')
    return private_python(source)


def prepare_models(manifest, provider, destination, cache, reuse_cache=None):
    release = script('prepare-windows-asr-release')
    chosen = ('tts-flow_lm-90m-amh-fp32',) if provider == 'small-tts' else (
        'edit-codec_llm-0.6b-amh', 'tts-codec_llm-0.6b-amh-fp16', 'codec-higgs_audio_v2-fp32', 'asr-wav2vec_ctc-1b-amh-int8_onnx')
    if destination.exists():
        raise ValueError('Use a new isolated model directory')
    destination.mkdir(parents=True)
    variants = {value['id']: value for value in manifest['variants']}
    for identity in chosen:
        variant = variants[identity]
        if reuse_cache:
            for entry in variant['files']:
                prior = reuse_cache / variant['folder'] / entry['path']
                target = cache / variant['folder'] / entry['path']
                if not target.exists() and release.valid_file(prior, entry):
                    target.parent.mkdir(parents=True, exist_ok=True)
                    shutil.copyfile(prior, target)
        downloaded = release.fetch_variant(manifest, variant, cache)
        target = destination / variant['folder']
        for entry in variant['files']:
            output = target / entry['path']
            output.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(downloaded / entry['path'], output)
            if not release.valid_file(output, entry):
                raise ValueError('Staged model failed its pinned hash/size check')
    (destination / 'manifest.json').write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--provider', choices=['small-tts', 'editing'], required=True)
    parser.add_argument('--base-runtime', type=Path, required=True)
    parser.add_argument('--source', type=Path, required=True)
    parser.add_argument('--models', type=Path, required=True)
    parser.add_argument('--cache', type=Path, required=True)
    parser.add_argument('--reuse-model-cache', type=Path)
    parser.add_argument('--manifest', type=Path, default=ROOT / 'apps/desktop/resources/studio-manifest.json')
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--version', required=True)
    parser.add_argument('--uv', default='uv')
    parser.add_argument('--prepare-only', action='store_true')
    args = parser.parse_args()
    if platform.system() != 'Windows' or platform.machine().lower() not in ('amd64', 'x86_64'):
        parser.error('Prepare and qualify this runtime on native Windows x64')
    release = script('prepare-windows-asr-release')
    manifest = release.checked_manifest(args.manifest)
    source = args.source.resolve()
    python = copy_interpreter(args.base_runtime, source)
    site = source / 'runtime/site-packages'
    cpu = args.provider == 'small-tts'
    requirements = ROOT / ('workers/tts/requirements-windows-x64.lock' if cpu else 'workers/editing/requirements-windows-x64.lock')
    env = {**os.environ, 'UV_CACHE_DIR': str(source.parent / 'uv-build-cache'), 'PYTHONDONTWRITEBYTECODE': '1',
           'UV_PYTHON_DOWNLOADS': 'never', 'PYTHONUTF8': '1'}
    print('Preparing private speech dependencies…', flush=True)
    subprocess.run([args.uv, 'pip', 'install', '--python', str(python), '--target', str(site),
                    '--only-binary', ':all:', '--requirements', str(requirements), '--extra-index-url',
                    'https://download.pytorch.org/whl/' + ('cpu' if cpu else 'cu128'),
                    '--index-strategy', 'unsafe-best-match'], check=True, env=env)
    if cpu:
        subprocess.run([args.uv, 'pip', 'install', '--python', str(python), '--target', str(site),
                        '--no-deps', POCKET_SOURCE], check=True, env=env)
    script('prepare-windows-asr-runtime').validate_wheels(source)
    info = json.loads(subprocess.check_output([str(python), '-I', '-B', '-c',
        'import importlib.metadata as m,json,sys;print(json.dumps({d.metadata["Name"]:d.version for d in m.distributions(path=[sys.argv[1]])}))', str(site)],
        text=True, encoding='utf-8'))
    metadata = {'schema_version': 1, 'os': 'windows', 'arch': 'x86_64', 'provider': args.provider,
                'python': python.relative_to(source).as_posix(), 'site_packages': 'runtime/site-packages',
                'requirements_sha256': hashlib.sha256(requirements.read_bytes()).hexdigest(),
                'distributions': info, 'pocket_source_commit': POCKET_COMMIT if cpu else None,
                'model_revision': manifest['hf_revision'], 'validated': False}
    (source / 'voice-runtime-preparation.json').write_text(json.dumps(metadata, indent=2) + '\n', encoding='utf-8')
    print('Preparing verified speech models…', flush=True)
    prepare_models(manifest, args.provider, args.models.resolve(), args.cache.resolve(), args.reuse_model_cache)
    if args.prepare_only:
        print('Prepared only; no inference qualification or release archive yet.', flush=True)
        return
    print('Running offline speech inference and packaging the qualified runtime…', flush=True)
    subprocess.run([sys.executable, str(ROOT / 'scripts/prepare-editing-runtime.py'), '--provider', args.provider,
                    '--source', str(source), '--python', python.relative_to(source).as_posix(),
                    '--models', str(args.models.resolve()), '--tts', 'tts-flow_lm-90m-amh-fp32' if cpu else 'tts-codec_llm-0.6b-amh-fp16',
                    '--output', str(args.output.resolve()), '--version', args.version], check=True, env=env)
    metadata['validated'] = True
    (source / 'voice-runtime-preparation.json').write_text(json.dumps(metadata, indent=2) + '\n', encoding='utf-8')


if __name__ == '__main__':
    main()
