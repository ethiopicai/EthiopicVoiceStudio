#!/usr/bin/env python3
"""Maintainer pack builder. Never installs dependencies, publishes or modifies source resources."""
import argparse
import gzip
import hashlib
import io
import json
from pathlib import Path
import re
import tarfile

MINIMUM_MACOS = '14.0'


def validate_runtime_tags(source, minimum=MINIMUM_MACOS):
    """Do not package a newer-OS wheel behind an older installer requirement."""
    baseline = tuple(map(int, minimum.split('.')))
    for wheel in (source / 'runtime/site-packages').glob('*.dist-info/WHEEL'):
        tags = re.findall(r'^Tag: .+$', wheel.read_text(), flags=re.MULTILINE)
        versions = [tuple(map(int, match)) for tag in tags
                    for match in re.findall(r'macosx_(\d+)_(\d+)_(?:arm64|universal2)', tag)]
        if versions and min(versions) > baseline:
            raise ValueError(f'{wheel.parent.name} needs macOS {min(versions)}; rebuild the runtime for the advertised {minimum} baseline')
        if not versions and not any(tag.endswith('-any') for tag in tags):
            raise ValueError(f'Unsupported platform wheel: {wheel.parent.name}')


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def entries(source, kind):
    prefix = 'bundle' if kind == 'model' else 'runtime'
    base = (source / prefix).resolve(strict=True)
    result = []
    for path in sorted((source / prefix).rglob('*')):
        relative = path.relative_to(source).as_posix()
        if '__pycache__' in path.parts or path.suffix == '.pyc' or path.name == '.DS_Store':
            continue
        actual = path.resolve(strict=True)
        if not actual.is_relative_to(base):
            raise ValueError(f'Resource link leaves its pack: {relative}')
        if path.is_dir():
            if path.is_symlink():
                raise ValueError(f'Directory symlinks are unsupported: {relative}')
            continue
        if not actual.is_file():
            raise ValueError(f'Only regular resource files are supported: {relative}')
        result.append({'path': relative, 'size': actual.stat().st_size,
                       'sha256': digest(actual), 'executable': bool(actual.stat().st_mode & 0o111)})
    if not result:
        raise ValueError('Resource pack is empty')
    return result


def build_pack(source, output, kind, version):
    files = entries(source, kind)
    interpreters = [f['path'] for f in files if re.fullmatch(r'runtime/python/[^/]+/bin/python3', f['path'])]
    if kind == 'runtime' and len(interpreters) != 1:
        raise ValueError('Expected one private macOS Python runtime')
    pack = {'schema_version': 1, 'kind': kind, 'model_version': version,
            'python': interpreters[0] if kind == 'runtime' else None,
            'site_packages': 'runtime/site-packages' if kind == 'runtime' else None,
            'files': files}
    manifest = json.dumps(pack, sort_keys=True, separators=(',', ':')).encode()
    name = f'amharic-asr-{version}-{kind}-macos-aarch64.tar.gz'
    target = output / name
    pending = target.with_suffix('.pending')
    with pending.open('wb') as raw, gzip.GzipFile(fileobj=raw, mode='wb', filename='', mtime=0, compresslevel=1) as compressed:
        with tarfile.open(fileobj=compressed, mode='w', format=tarfile.PAX_FORMAT) as archive:
            info = tarfile.TarInfo('pack.json')
            info.size = len(manifest)
            info.mode = 0o644
            archive.addfile(info, io.BytesIO(manifest))
            for file in files:
                info = tarfile.TarInfo(file['path'])
                info.size = file['size']
                info.mode = 0o755 if file['executable'] else 0o644
                with (source / file['path']).open('rb') as stream:
                    archive.addfile(info, stream)
    pending.replace(target)
    return {'kind': kind, 'filename': name, 'sha256': digest(target),
            'size': target.stat().st_size, 'unpacked_size': len(manifest) + sum(f['size'] for f in files)}


def make_catalog(packs, version, repository, revision):
    if not re.fullmatch(r'ethiopic/[A-Za-z0-9][A-Za-z0-9_.-]*', repository):
        raise ValueError('Use a public repository under ethiopic/')
    if not re.fullmatch(r'[0-9a-fA-F]{40}', revision):
        raise ValueError('Pin the full 40-character HF commit, not main or latest')
    artifacts = []
    for pack in packs:
        item = {key: pack[key] for key in ('kind', 'sha256', 'size', 'unpacked_size')}
        item['url'] = f"https://huggingface.co/{repository}/resolve/{revision}/{pack['filename']}"
        artifacts.append(item)
    return {'schema_version': 1, 'releases': [{'os': 'macos', 'arch': 'aarch64',
            'minimum_macos': MINIMUM_MACOS, 'model_version': version, 'packs': artifacts}]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source', type=Path, default=Path('.local-asr'))
    parser.add_argument('--output', type=Path, default=Path('.release/asr-macos'))
    parser.add_argument('--catalog-only', action='store_true', help='Use previously built packs; does not upload them')
    parser.add_argument('--repository')
    parser.add_argument('--revision', help='Immutable HF commit after resources have been uploaded')
    parser.add_argument('--clearance-record', type=Path, help='Owner-approved redistribution/provenance decision file')
    args = parser.parse_args()
    source, output = args.source.resolve(), args.output.resolve()
    if output.is_relative_to(source):
        parser.error('Pack output must be outside the source resources')
    output.mkdir(parents=True, exist_ok=True)
    metadata = output / 'pack-build.json'
    if args.catalog_only:
        data = json.loads(metadata.read_text())
        if data.get('minimum_macos') != MINIMUM_MACOS:
            raise ValueError('Rebuild pack metadata with the current macOS compatibility checks before creating a catalogue')
        version, packs = data['model_version'], data['packs']
        for pack in packs:
            if digest(output / pack['filename']) != pack['sha256']:
                raise ValueError('A built pack changed; rebuild before creating the catalogue')
    else:
        validate_runtime_tags(source)
        version = json.loads((source / 'bundle/model/version.json').read_text())['model_version']
        if not re.fullmatch(r'[A-Za-z0-9_.-]{1,64}', version):
            raise ValueError('Invalid model version')
        packs = []
        for kind in ('model', 'runtime'):
            print(f'Packing {kind}; source resources remain unchanged…', flush=True)
            packs.append(build_pack(source, output, kind, version))
        metadata.write_text(json.dumps({'schema_version': 1, 'minimum_macos': MINIMUM_MACOS, 'model_version': version, 'packs': packs}, indent=2) + '\n')
    if args.repository or args.revision:
        if not args.repository or not args.revision or not args.clearance_record:
            parser.error('A release catalogue requires repository, immutable revision and a clearance record')
        record = args.clearance_record.resolve(strict=True)
        if record.stat().st_size == 0:
            raise ValueError('Redistribution decision record is empty')
        catalog = make_catalog(packs, version, args.repository, args.revision)
        (output / 'asr-catalog.json').write_text(json.dumps(catalog, indent=2) + '\n')
        (output / 'publication-evidence.json').write_text(json.dumps({
            'clearance_record_sha256': digest(record), 'repository': args.repository,
            'revision': args.revision, 'model_version': version,
            'note': 'This records the supplied owner decision; it is not an automated legal approval.'}, indent=2) + '\n')
        print('Pinned catalogue prepared. Verify published artifacts before copying it into the app resources.')
    else:
        print('Local draft packs prepared. Not published or approved for redistribution; no release catalogue was created.')


if __name__ == '__main__':
    main()
