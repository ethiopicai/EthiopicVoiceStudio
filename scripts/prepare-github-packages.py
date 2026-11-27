#!/usr/bin/env python3
"""Stage reviewed preview installers and validate the exact assets for a draft GitHub release."""
import argparse
import hashlib
import importlib.util
import json
from pathlib import Path
import re
import shutil
import struct

ROOT = Path(__file__).resolve().parents[1]
MAX_ASSET_BYTES = 2 * 1024**3
TARGETS = {
    'aarch64-apple-darwin': ('macos', 'aarch64', 'arm64', 'dmg', 'preview'),
    'x86_64-apple-darwin': ('macos', 'x86_64', 'x64', 'dmg', 'interface-preview'),
    'x86_64-pc-windows-msvc': ('windows', 'x86_64', 'x64', 'exe', 'preview'),
}


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def identity(target, version):
    if target not in TARGETS or not re.fullmatch(r'\d+\.\d+\.\d+(?:-[A-Za-z0-9.-]+)?', version):
        raise ValueError('Unsupported package target or version')
    system, arch, cpu, extension, support = TARGETS[target]
    stem = f'EthiopicAI-Voice-Studio-{version}-{system}-{cpu}-preview'
    return system, arch, support, stem, stem + '.' + extension


def check_binary(path, target):
    with path.open('rb') as stream:
        header = stream.read(64)
        if target.endswith('apple-darwin'):
            if len(header) < 8 or header[:4] != b'\xcf\xfa\xed\xfe':
                raise ValueError('Expected a thin 64-bit Mach-O executable')
            wanted = 0x0100000C if target.startswith('aarch64') else 0x01000007
            if struct.unpack('<I', header[4:8])[0] != wanted:
                raise ValueError('Mac executable architecture does not match the package')
        else:
            if len(header) != 64 or header[:2] != b'MZ':
                raise ValueError('Expected a Windows executable')
            stream.seek(struct.unpack('<I', header[60:64])[0])
            pe = stream.read(6)
            if len(pe) != 6 or pe[:4] != b'PE\0\0' or struct.unpack('<H', pe[4:])[0] != 0x8664:
                raise ValueError('Windows application executable must be AMD64')


def stage(target, commit, output, root=ROOT):
    if not re.fullmatch('[0-9a-f]{40}', commit):
        raise ValueError('Package provenance requires an exact source commit')
    config = json.loads((root / 'apps/desktop/src-tauri/tauri.conf.json').read_text(encoding='utf-8'))
    version, product = config['version'], config['productName']
    system, arch, support, stem, filename = identity(target, version)
    release = root / 'target' / target / 'release'
    if system == 'macos':
        directory = root / '.release' / f'macos-{version}{"-intel" if arch == "x86_64" else ""}-candidate'
        status = json.loads((directory / 'build-status.json').read_text(encoding='utf-8'))
        if (status['version'], status['arch'], status.get('target'), status['public_release']) != (version, arch, target, False):
            raise ValueError('Mac candidate status does not match this package')
        name = f'EthiopicAI-Voice-Studio-{version}-macos-{"arm64" if arch == "aarch64" else "x64"}-candidate.dmg'
        source = directory / name
        if digest(source) != status['sha256']:
            raise ValueError('Mac disk image differs from its verified candidate')
        binary = release / 'bundle/macos' / (product + '.app') / 'Contents/MacOS/ethiopic-voice-studio'
    else:
        source = release / 'bundle/nsis' / f'{product}_{version}_x64-setup.exe'
        binary = release / 'ethiopic-voice-studio.exe'
    check_binary(binary, target)
    spec = importlib.util.spec_from_file_location('build_privacy', ROOT / 'scripts/build-privacy.py')
    privacy = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(privacy)
    privacy.reject_build_paths(binary, root)
    if not 0 < source.stat().st_size < MAX_ASSET_BYTES:
        raise ValueError('Installer exceeds GitHub release asset limits or is empty')
    output.mkdir(parents=True, exist_ok=True)
    installer, metadata = output / filename, output / (stem + '.json')
    if any(p.exists() for p in [installer, metadata, output / (filename + '.sha256')]):
        raise ValueError('Refusing to replace an already staged package')
    shutil.copyfile(source, installer)
    sha = digest(installer)
    (output / (filename + '.sha256')).write_text(f'{sha}  {filename}\n', encoding='utf-8')
    record = {'schema_version': 1, 'version': version, 'target': target, 'platform': system, 'arch': arch,
              'source_commit': commit, 'support_stage': support, 'public_release': False,
              'signing': 'ad-hoc' if system == 'macos' else 'unsigned',
              'installer': filename, 'bytes': installer.stat().st_size, 'sha256': sha}
    metadata.write_text(json.dumps(record, indent=2) + '\n', encoding='utf-8')
    return record


def draft(assets, output, commit, tag=None):
    if not re.fullmatch('[0-9a-f]{40}', commit):
        raise ValueError('Draft requires an exact source commit')
    records = [json.loads(p.read_text(encoding='utf-8')) for p in assets.glob('*.json')]
    if len(records) != 3 or {r['target'] for r in records} != set(TARGETS):
        raise ValueError('Draft requires exactly one package for each supported build target')
    versions = {r['version'] for r in records}
    if len(versions) != 1:
        raise ValueError('Packages must have the same version')
    version = next(iter(versions))
    tag = tag or f'v{version}'
    if tag != f'v{version}':
        raise ValueError('Release tag must match the packaged application version')
    expected = set()
    for record in records:
        system, arch, support, stem, name = identity(record['target'], version)
        if (record['schema_version'], record['source_commit'], record['public_release'], record['support_stage'], record['platform'], record['arch'], record['installer']) != (1, commit, False, support, system, arch, name):
            raise ValueError('Package metadata does not match this source or preview target')
        if record['signing'] != ('ad-hoc' if system == 'macos' else 'unsigned'):
            raise ValueError('Unexpected preview signing declaration')
        path = assets / name
        if path.is_symlink() or not 0 < path.stat().st_size < MAX_ASSET_BYTES:
            raise ValueError('Invalid release installer')
        if path.stat().st_size != record['bytes'] or digest(path) != record['sha256']:
            raise ValueError('Release installer checksum does not match its package metadata')
        checksum = assets / (name + '.sha256')
        if checksum.read_text(encoding='utf-8') != f'{record["sha256"]}  {name}\n':
            raise ValueError('Release checksum file does not match its package')
        expected.update([name, name + '.sha256', stem + '.json'])
    if {p.name for p in assets.iterdir()} != expected:
        raise ValueError('Unexpected files in release assets; refusing to publish them')
    output.mkdir(parents=True, exist_ok=True)
    notes = f'''# EthiopicAI Voice Studio {version} — preview

- Apple Silicon Mac: macOS 14 or later.
- Windows: 64-bit Windows 10 (build 19041 or later) or Windows 11.
- Intel Mac: macOS 14 or later. **Interface testing only; voice-tool setup is not available for Intel Macs yet.**

Download the installer for your computer. Voice models and compatible runtimes are downloaded separately from Hugging Face after you review and approve setup.

These are review packages: Mac builds are ad-hoc signed and not notarized; the Windows installer is unsigned. These packages are not a signed production release.

Source commit: `{commit}`. SHA-256 checksums and per-platform package metadata are included.
'''
    (output / 'RELEASE_NOTES.md').write_text(notes, encoding='utf-8')
    (output / 'release-tag.txt').write_text(tag + '\n', encoding='utf-8')
    (output / 'release-files.txt').write_text(''.join(str((assets / name).resolve()) + '\n' for name in sorted(expected)), encoding='utf-8')
    return tag


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='command', required=True)
    package = commands.add_parser('stage')
    package.add_argument('--target', choices=TARGETS, required=True)
    package.add_argument('--commit', required=True)
    package.add_argument('--output', type=Path, required=True)
    release = commands.add_parser('draft')
    release.add_argument('--assets', type=Path, required=True)
    release.add_argument('--output', type=Path, required=True)
    release.add_argument('--commit', required=True)
    release.add_argument('--tag')
    args = parser.parse_args()
    if args.command == 'stage':
        print(json.dumps(stage(args.target, args.commit, args.output)))
    else:
        print(draft(args.assets, args.output, args.commit, args.tag))


if __name__ == '__main__':
    main()
