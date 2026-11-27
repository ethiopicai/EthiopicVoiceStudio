#!/usr/bin/env python3
"""Build Studio's master release inventory without changing provider manifests.

This is a deterministic local draft, not a signed/published update feed. Existing
provider formats are retained for incremental migration. No files are downloaded,
executed or uploaded. --release-ready explains outstanding distribution gates.
"""
import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath
import re
from urllib.parse import urlparse

ROOT = Path(__file__).resolve().parents[1]
RESOURCES = ROOT / 'apps/desktop/resources'
SOURCES = {
    'models': 'studio-manifest.json',
    'recognition_packages': 'asr-catalog.json',
    'recognition_routes': 'asr-routes.json',
    'speech_runtimes': 'tts-runtimes.json',
    'editing_runtimes': 'editing-runtimes.json',
    'noise_reduction': 'noise-reduction.json',
}


def safe_path(value):
    path = PurePosixPath(value)
    return bool(path.parts) and path.as_posix() == value and not path.is_absolute() and '..' not in path.parts and '\\' not in value and not any(ord(c) < 32 for c in value)


def validate_artifact(artifact):
    if artifact['kind'] == 'legacy-model-file':
        if set(artifact)!={'kind','component','path','migration_key','bytes','sha256'}:
            raise ValueError('Invalid migration artifact fields')
        bridge=json.loads((RESOURCES/'naming-migration/transport.json').read_text(encoding='utf-8'))
        source=bridge['file_sources'].get(artifact['migration_key'],{})
        if source.get('sha256')!=artifact['sha256']:
            raise ValueError('Unverified naming migration source')
        validate_artifact({**artifact,'kind':'model-file','url':source['url']})
        return
    if artifact['kind'] == 'bundled-model-file':
        if set(artifact)!={'kind','component','path','bytes','sha256'}:
            raise ValueError('Invalid bundled artifact fields')
        sha=artifact['sha256'];asset=RESOURCES/'naming-migration'/f'{sha}.asset'
        if not re.fullmatch('[0-9a-f]{64}',sha) or not asset.is_file() or asset.stat().st_size!=artifact['bytes'] or hashlib.sha256(asset.read_bytes()).hexdigest()!=sha:
            raise ValueError('Invalid bundled model support file')
        return
    url = urlparse(artifact['url'])
    if url.scheme != 'https' or url.username or url.password or url.port not in (None, 443) or url.query or url.fragment:
        raise ValueError('Artifact URLs must be public, pinned HTTPS locations')
    parts = url.path.strip('/').split('/')
    if url.hostname == 'huggingface.co':
        repo_ok = '/'.join(parts[:2]) in ('ethiopic/ethiopic-studio-models', 'inworld/voice-isolator')
        archive_ok = artifact['kind'] in ('model', 'runtime') and len(parts) == 5 and parts[0] == 'ethiopic' and bool(re.fullmatch(r'[A-Za-z0-9_.-]+',parts[1])) and parts[-1].endswith('.tar.gz')
        if len(parts) < 5 or not (repo_ok or archive_ok) or parts[2] != 'resolve' or not re.fullmatch('[0-9a-f]{40}', parts[3]):
            raise ValueError('Model URL must use an approved repository and immutable commit')
    elif url.hostname == 'files.pythonhosted.org':
        if not url.path.startswith('/packages/') or not url.path.endswith('.whl'):
            raise ValueError('Runtime dependencies must be pinned wheel files')
    else:
        raise ValueError('Unapproved artifact host')
    if '%' in url.path or not safe_path(url.path.lstrip('/')):
        raise ValueError('Unsafe artifact path')
    if not re.fullmatch('[0-9a-f]{64}', artifact['sha256']):
        raise ValueError('Artifact SHA-256 required')
    size = artifact['bytes']
    if isinstance(size, bool) or not isinstance(size, int) or not 0 <= size <= 8*1024**3:
        raise ValueError('Invalid artifact byte size')


def artifacts_for(catalogs):
    artifacts = {}
    models = catalogs['models']
    for variant in models['variants']:
        for file in variant['files']:
            key = f"model:{variant['id']}:{file['path']}"
            if key in artifacts:
                raise ValueError('Duplicate model artifact')
            bridge=json.loads((RESOURCES/'naming-migration/transport.json').read_text(encoding='utf-8'))
            relative=f"{variant['id']}/{file['path']}"
            support=(bridge['bundled_files'] if models['hf_revision']==bridge['revision'] else
                     bridge.get('current_bundled_files',{}) if models['hf_revision']==bridge.get('public_revision') else {})
            bundled=(models['manifest_version']==2 and support.get(relative)==file['sha256'])
            legacy=bridge['file_sources'].get(relative,{}) if models['hf_revision']==bridge['revision'] else {}
            artifact={'kind':'bundled-model-file' if bundled else 'legacy-model-file' if legacy.get('sha256')==file['sha256'] else 'model-file',
                      'component':variant['id'],'path':file['path'],'bytes':file['bytes'],'sha256':file['sha256']}
            if artifact['kind']=='legacy-model-file':artifact['migration_key']=relative
            elif not bundled:artifact['url']=f"https://huggingface.co/{models['hf_repo']}/resolve/{models['hf_revision']}/{variant['folder']}/{file['path']}"
            artifacts[key]=artifact
    for file in catalogs['noise_reduction']['files']:
        key = 'noise-reduction:' + file['name']
        if key in artifacts:
            raise ValueError('Duplicate noise reduction artifact')
        artifacts[key] = {'kind': file['kind'], 'component': 'recognition-noise-reduction',
                          'path': file['name'], 'url': file['url'], 'bytes': file['bytes'], 'sha256': file['sha256']}
    for name in ['recognition_packages', 'speech_runtimes', 'editing_runtimes']:
        for release in catalogs[name]['releases']:
            files = release['packs'] if name == 'recognition_packages' else [release['artifact']]
            for file in files:
                if not 0 < file['size'] or not 0 < file['unpacked_size'] <= 12*1024**3:
                    raise ValueError('Invalid runtime archive size')
                key = f"runtime:{name}:{release['os']}:{release['arch']}:{release.get('profile', '')}:{file['sha256']}"
                artifacts[key] = {'kind': file['kind'], 'component': name, 'path': urlparse(file['url']).path.rsplit('/',1)[-1],
                                  'url': file['url'], 'bytes': file['size'], 'sha256': file['sha256']}
    return artifacts


def validate(catalog):
    if catalog.get('schema_version') != 1 or catalog.get('channel') not in ('preview', 'stable'):
        raise ValueError('Unsupported catalog schema/channel')
    if isinstance(catalog.get('sequence'), bool) or not isinstance(catalog.get('sequence'), int) or catalog['sequence'] < 0:
        raise ValueError('Invalid catalog sequence')
    if not re.fullmatch(r'\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?', catalog['minimum_app_version']):
        raise ValueError('Minimum app version must be SemVer')
    models = catalog['catalogs']['models']
    if models['hf_repo'] != 'ethiopic/ethiopic-studio-models' or not re.fullmatch('[0-9a-f]{40}', models['hf_revision']):
        raise ValueError('Invalid model release source')
    variants = {v['id']: v for v in models['variants']}
    if len(variants) != len(models['variants']):
        raise ValueError('Duplicate model IDs')
    for variant in variants.values():
        if not safe_path(variant['folder']) or any(not safe_path(f['path']) for f in variant['files']):
            raise ValueError('Unsafe model file path')
        if sum(f['bytes'] for f in variant['files']) != variant['total_bytes']:
            raise ValueError('Model size does not match its declared files')
        if any(dep not in variants for dep in variant['requires'] + variant.get('requires_one_of', [])):
            raise ValueError('Missing model dependency')
        if variant['id'] in variant.get('requires_one_of', []):
            raise ValueError('Model cannot depend on itself')
    def visit(key, stack):
        if key in stack:
            raise ValueError('Cyclic model dependency')
        for dep in variants[key]['requires']:
            visit(dep, stack | {key})
    for key in variants:
        visit(key, set())
    for bundle in models['bundles'].values():
        for key in ['asr', 'asr_live', 'corrector', 'tts', 'tts_optin', 'edit', 'edit_optin']:
            if bundle.get(key) and bundle[key] not in variants:
                raise ValueError('Tier refers to a missing model')
    expected = artifacts_for(catalog['catalogs'])
    if catalog['artifacts'] != expected:
        raise ValueError('Artifact index drifted from its provider catalogs')
    for artifact in expected.values():
        validate_artifact(artifact)
    policy = catalog['update_policy']
    for field in ['catalog_endpoint', 'catalog_signature_endpoint', 'app_update_endpoint']:
        url = urlparse(policy[field])
        if url.scheme != 'https' or url.hostname != 'ethiopic.ai' or url.username or url.password or url.query or url.fragment or url.port not in (None,443):
            raise ValueError('Update discovery must stay on the first-party HTTPS host')
    if policy['channel'] != catalog['channel'] or policy['check_interval_seconds'] < 3600:
        raise ValueError('Invalid update discovery policy')
    return catalog


def build(resources=RESOURCES):
    catalogs = {name: json.loads((resources / file).read_text(encoding='utf-8')) for name, file in SOURCES.items()}
    version = json.loads((resources.parent / 'src-tauri/tauri.conf.json').read_text(encoding='utf-8'))['version']
    policy = json.loads((resources / 'update-policy.json').read_text(encoding='utf-8'))
    inventory = [
        {'id': v['id'], 'component': v['component'], 'version': v.get('model_version'),
         'artifact_status': 'available-with-naming-migration', 'family':v.get('family'), 'release':v.get('release'), 'license': v['license'], 'catalog': 'models',
         'requires': v['requires'], 'requires_one_of': v.get('requires_one_of', [])}
        for v in catalogs['models']['variants']
    ]
    inventory.extend([
        {'id': 'recognition-noise-reduction', 'component': 'noise-reduction', 'version': 'v1-realtime',
         'artifact_status': 'published', 'license': 'Apache-2.0', 'catalog': 'noise_reduction',
         'platforms': ['macos-aarch64'], 'requires_base_runtime': {'python': '3.11', 'torch': '2.9.1'}},
        {'id': 'legacy-amharic-ctc', 'component': 'asr', 'version': 'v0.2',
         'artifact_status': 'local-development-installation', 'catalog': 'recognition_packages',
         'managed_download_available': False},
        {'id': 'voice-cleanup-reuse', 'component': 'cleanup', 'version': 'nemotronlabs-re-use-31f82f2-cpu-v1',
         'artifact_status': 'local-validated-provider', 'license': 'OpenMDW-1.1',
         'upstream_repository': 'nvidia/NVIDIA-NemotronLabs-RE-USE',
         'upstream_revision': '31f82f2efad1095034492f236e8dac9239ba713b', 'managed_download_available': False},
    ])
    return validate({
        'schema_version': 1, 'product': 'ethiopic-voice-studio', 'channel': policy['channel'],
        'release_status': 'draft', 'sequence': 0, 'published_at': None, 'expires_at': None,
        'minimum_app_version': version, 'bundled_app_version': version,
        'supported_app_platforms': ['macos-aarch64', 'windows-x86_64'],
        'update_policy': policy, 'inventory': inventory, 'catalogs': catalogs,
        'artifacts': artifacts_for(catalogs),
        'source_sha256': {file: hashlib.sha256((resources/file).read_bytes()).hexdigest() for file in SOURCES.values()},
    })


def release_blockers(catalog):
    blockers = []
    if catalog['release_status'] != 'published' or catalog['sequence'] < 1 or not catalog['published_at'] or not catalog['expires_at']:
        blockers.append('Catalog is a local draft; assign release sequence and validity dates when publishing.')
    policy = catalog['update_policy']
    if not policy['trusted_catalog_keys']:
        blockers.append('Embed the trusted catalog public key and verify detached signatures before remote activation.')
    if not policy['app_updater_configured']:
        blockers.append('Configure Tauri updater signing and publish signed application update artifacts.')
    for name in ['recognition_packages', 'speech_runtimes', 'editing_runtimes']:
        available = {f"{release['os']}-{release['arch']}" for release in catalog['catalogs'][name]['releases']}
        for platform in catalog['supported_app_platforms']:
            if platform not in available:
                blockers.append(f'No published platform runtime releases: {name} ({platform}).')
    if not catalog['catalogs']['recognition_routes']['releases']:
        blockers.append('Publish recognition routes matching the selected model/runtime release.')
    if any(v['id'] == 'voice-cleanup-reuse' and not v['managed_download_available'] for v in catalog['inventory']):
        blockers.append('Voice Cleanup has no published managed model/runtime pack.')
    return blockers


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, default=RESOURCES/'release-catalog.json')
    parser.add_argument('--check', action='store_true')
    parser.add_argument('--release-ready', action='store_true')
    args = parser.parse_args()
    catalog = build()
    encoded = json.dumps(catalog, ensure_ascii=False, indent=2) + '\n'
    if args.check:
        if args.output.read_text(encoding='utf-8') != encoded:
            raise SystemExit('Master release catalog is stale. Run scripts/build-release-catalog.py.')
    else:
        args.output.write_text(encoded, encoding='utf-8')
    print(f"Validated {len(catalog['inventory'])} model/provider records and {len(catalog['artifacts'])} downloadable files.")
    if args.release_ready:
        blockers = release_blockers(catalog)
        if blockers:
            raise SystemExit('\n'.join(blockers))
