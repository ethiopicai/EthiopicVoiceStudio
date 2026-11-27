#!/usr/bin/env python3
"""Build pinned Windows recognition model/runtime archives; does not publish them."""
import argparse
import gzip
import hashlib
import io
import json
from pathlib import Path
import re
import shutil
import tarfile
import time
import urllib.error
import urllib.request

PROFILES = ('cpu-low', 'cpu-mid', 'cpu-high', 'gpu-low', 'gpu-pascal', 'gpu-modern')


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def safe_path(value):
    if not isinstance(value, str) or not value or any(c in value for c in '\\:\0'):
        return False
    return not value.startswith('/') and all(p not in ('', '.', '..') for p in value.split('/'))


def checked_manifest(path):
    manifest = json.loads(path.read_text(encoding='utf-8'))
    if manifest.get('manifest_version') not in (1, 2):
        raise ValueError('Unsupported model manifest')
    if not re.fullmatch(r'ethiopic/[A-Za-z0-9][A-Za-z0-9_.-]*', manifest.get('hf_repo', '')):
        raise ValueError('Use the public first-party HF repository')
    if not re.fullmatch(r'[a-fA-F0-9]{40}', manifest.get('hf_revision', '')):
        raise ValueError('Pin an immutable HF commit')
    for variant in manifest['variants']:
        if not safe_path(variant['folder']):
            raise ValueError('Unsafe variant directory')
        for item in variant['files']:
            if not safe_path(item['path']) or not re.fullmatch(r'[a-fA-F0-9]{64}', item['sha256']) or item['bytes'] <= 0:
                raise ValueError('Invalid pinned model file')
    return manifest


def valid_file(path, item):
    return path.is_file() and not path.is_symlink() and path.stat().st_size == item['bytes'] and digest(path) == item['sha256']


def fetch_variant(manifest, variant, cache):
    folder = cache / variant['folder']
    for item in variant['files']:
        target = folder / item['path']
        if valid_file(target, item):
            continue
        target.parent.mkdir(parents=True, exist_ok=True)
        partial = target.with_name(target.name + '.download-part')
        url = f"https://huggingface.co/{manifest['hf_repo']}/resolve/{manifest['hf_revision']}/{variant['folder']}/{item['path']}"
        for attempt in range(4):
            try:
                offset = partial.stat().st_size if partial.exists() else 0
                if offset >= item['bytes']:
                    partial.unlink()
                    offset = 0
                request = urllib.request.Request(url, headers={'User-Agent': 'EthiopicStudio-Maintainer/1', **({'Range': f'bytes={offset}-'} if offset else {})})
                with urllib.request.urlopen(request, timeout=90) as response:
                    append = offset > 0 and response.status == 206
                    if append and not response.headers.get('Content-Range', '').startswith(f'bytes {offset}-'):
                        raise ValueError('Download resume range mismatch')
                    with partial.open('ab' if append else 'wb') as output:
                        shutil.copyfileobj(response, output, 1024 * 1024)
                if not valid_file(partial, item):
                    raise ValueError('Downloaded model file failed its SHA-256/size check')
                partial.replace(target)
                print(f"Verified {variant['id']}/{item['path']} ({item['bytes']} bytes)", flush=True)
                break
            except (OSError, ValueError, urllib.error.URLError):
                if attempt == 3:
                    raise
                time.sleep(min(2 ** attempt, 4))
    return folder


def recognition_config(manifest, profile):
    if profile not in PROFILES:
        raise ValueError('Unsupported Windows hardware profile')
    variants = {v['id']: v for v in manifest['variants']}
    bundle = manifest['bundles'][profile]
    result = {'schema_version': 1}
    for task, key in [('transcription', 'asr'), ('dictation', 'asr_live')]:
        variant = variants[bundle[key]]
        torch = variant['id'].endswith('-fp16')
        config = {'variant': variant['id'], 'backend': 'torch' if torch else 'onnx',
                  'folder': variant['folder'], 'device': 'cuda' if torch else 'cpu',
                  'dtype': 'float16' if torch else 'int8', 'model_version': variant['model_version'],
                  'model': variant['model'], 'threads': 8}
        if task == 'dictation' and not torch:
            config.update(context_sec=2, update_sec=0.8)
        result[task] = config
    return result


def prepare_models(manifest, profile, cache, source):
    bundle = source / 'bundle'
    if bundle.exists():
        raise ValueError('Model staging directory already exists; use a fresh source directory')
    config = recognition_config(manifest, profile)
    selected = {config[t]['variant'] for t in ('transcription', 'dictation')}
    variants = {v['id']: v for v in manifest['variants']}
    bundle.mkdir(parents=True)
    for identity in sorted(selected):
        variant = variants[identity]
        downloaded = fetch_variant(manifest, variant, cache)
        for item in variant['files']:
            target = bundle / variant['folder'] / item['path']
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(downloaded / item['path'], target)
            if not valid_file(target, item):
                raise ValueError('Model staging verification failed')
        # Public model folders intentionally contain only manifest-listed files.
        # This is app-generated installation metadata, not a source variant edit.
        metadata = variant
        (bundle / variant['folder'] / 'variant.json').write_text(
            json.dumps(metadata, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
    (bundle / 'recognition.json').write_text(json.dumps(config, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
    return config


def entries(source, kind):
    prefix = 'runtime' if kind == 'runtime' else 'bundle'
    base = (source / prefix).resolve(strict=True)
    files = []
    for path in sorted((source / prefix).rglob('*')):
        if '__pycache__' in path.parts or path.suffix == '.pyc' or path.name == '.DS_Store':
            continue
        relative = path.relative_to(source).as_posix()
        actual = path.resolve(strict=True)
        if not actual.is_relative_to(base):
            raise ValueError(f'Resource link leaves its pack: {relative}')
        if path.is_dir():
            if path.is_symlink():
                raise ValueError('Directory symlinks are unsupported')
            continue
        if not actual.is_file():
            raise ValueError('Only regular resource files are supported')
        files.append({'path': relative, 'size': actual.stat().st_size, 'sha256': digest(actual),
                      'executable': path.suffix.lower() == '.exe'})
    if not files:
        raise ValueError('Resource pack is empty')
    return files


def build_pack(source, output, kind, version, profile):
    if kind not in ('model', 'runtime') or not re.fullmatch(r'[A-Za-z0-9_.-]{1,64}', version) or not re.fullmatch(r'[A-Za-z0-9_-]{1,64}', profile):
        raise ValueError('Unsafe pack identity')
    files = entries(source, kind)
    interpreters = [f['path'] for f in files if re.fullmatch(r'runtime/python/[^/]+/python.exe', f['path'])]
    if kind == 'runtime' and len(interpreters) != 1:
        raise ValueError('Expected one portable Windows Python runtime')
    pack = {'schema_version': 1, 'kind': kind, 'model_version': version,
            'python': interpreters[0] if kind == 'runtime' else None,
            'site_packages': 'runtime/site-packages' if kind == 'runtime' else None, 'files': files}
    manifest = json.dumps(pack, sort_keys=True, separators=(',', ':')).encode()
    output.mkdir(parents=True, exist_ok=True)
    filename = f'amharic-asr-{version}-{kind}-windows-x86_64-{profile}.tar.gz'
    target = output / filename
    pending = target.with_suffix('.pending')
    with pending.open('wb') as raw, gzip.GzipFile(fileobj=raw, mode='wb', filename='', mtime=0, compresslevel=1) as compressed:
        with tarfile.open(fileobj=compressed, mode='w', format=tarfile.PAX_FORMAT) as archive:
            info = tarfile.TarInfo('pack.json')
            info.size, info.mode = len(manifest), 0o644
            archive.addfile(info, io.BytesIO(manifest))
            for item in files:
                info = tarfile.TarInfo(item['path'])
                info.size, info.mode = item['size'], 0o755 if item['executable'] else 0o644
                with (source / item['path']).open('rb') as stream:
                    archive.addfile(info, stream)
    pending.replace(target)
    return {'kind': kind, 'filename': filename, 'sha256': digest(target), 'size': target.stat().st_size,
            'unpacked_size': len(manifest) + sum(f['size'] for f in files)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--manifest', type=Path, required=True)
    parser.add_argument('--cache', type=Path, required=True)
    parser.add_argument('--source', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--runtime-onnx', type=Path, required=True)
    parser.add_argument('--runtime-cuda', type=Path)
    parser.add_argument('--profile', action='append', choices=PROFILES)
    args = parser.parse_args()
    manifest = checked_manifest(args.manifest)
    profiles = args.profile or ['cpu-high', 'cpu-mid', 'cpu-low']
    metadata_path = args.output / 'pack-build-windows.json'
    prepared = json.loads(metadata_path.read_text(encoding='utf-8'))['releases'] if metadata_path.exists() else []
    if any(release['profile'] in profiles for release in prepared):
        raise ValueError('A requested hardware profile already has release metadata')
    runtimes = {}
    for profile in profiles:
        config = prepare_models(manifest, profile, args.cache.resolve(), args.source.resolve() / profile)
        version = config['transcription']['model_version']
        if version != config['dictation']['model_version']:
            raise ValueError('Recognition model releases must match')
        backend = 'cuda' if config['transcription']['backend'] == 'torch' else 'onnx'
        runtime_source = args.runtime_cuda if backend == 'cuda' else args.runtime_onnx
        if runtime_source is None:
            raise ValueError('The selected hardware profile needs a validated CUDA runtime')
        metadata = json.loads((runtime_source / 'runtime-preparation.json').read_text(encoding='utf-8'))
        if metadata['backend'] != backend or metadata['os'] != 'windows' or metadata['arch'] != 'x86_64':
            raise ValueError('Runtime backend/platform mismatch')
        if metadata.get('app_local_crt_verified') is not True or not metadata.get('visual_cpp_runtime'):
            raise ValueError('Runtime must include and verify its pinned private Microsoft CRT')
        key = (backend, version)
        if key not in runtimes:
            runtimes[key] = build_pack(runtime_source, args.output, 'runtime', version, backend)
        model = build_pack(args.source.resolve() / profile, args.output, 'model', version, profile)
        prepared.append({'os': 'windows', 'arch': 'x86_64', 'minimum_windows': '10.0.19041',
                         'profile': profile, 'model_version': version, 'hf_revision': manifest['hf_revision'],
                         'transcription': config['transcription']['variant'], 'dictation': config['dictation']['variant'],
                         'packs': [model, runtimes[key]]})
        metadata_path.write_text(json.dumps({'schema_version': 1, 'releases': prepared}, indent=2) + '\n', encoding='utf-8')
        print(f'Prepared {profile}; archives are not published.', flush=True)


if __name__ == '__main__':
    main()
