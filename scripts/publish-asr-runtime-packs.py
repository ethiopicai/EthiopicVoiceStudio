#!/usr/bin/env python3
"""Publish qualified Windows recognition archives and emit immutable download catalogs."""
import argparse
import hashlib
import json
from pathlib import Path
import re

REPOSITORY = 'ethiopic/ethiopic-studio-models'


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def qualified_archives(source, evidence):
    metadata = json.loads((source / 'pack-build-windows.json').read_text(encoding='utf-8'))
    if metadata.get('schema_version') != 1 or not metadata.get('releases'):
        raise ValueError('Expected nonempty Windows release metadata')
    if evidence.get('schema_version') != 1:
        raise ValueError('Expected Windows qualification evidence')
    archives, profiles = {}, set()
    for release in metadata['releases']:
        profile = release['profile']
        if profile in profiles or profile not in ('cpu-low', 'cpu-mid', 'cpu-high', 'gpu-low', 'gpu-pascal', 'gpu-modern'):
            raise ValueError('Duplicate or unsupported hardware profile')
        profiles.add(profile)
        if (release['os'], release['arch'], release['minimum_windows']) != ('windows', 'x86_64', '10.0.19041'):
            raise ValueError('Unexpected runtime platform')
        check = evidence.get('profiles', {}).get(profile, {})
        if check.get('offline_inference') is not True or check.get('guided_install') is not True:
            raise ValueError('Every published profile needs offline inference and native installer qualification')
        if not re.fullmatch('[0-9a-f]{40}', release['hf_revision']):
            raise ValueError('Model source must be immutable')
        if sorted(pack['kind'] for pack in release['packs']) != ['model', 'runtime']:
            raise ValueError('Expected one model and one runtime pack')
        for pack in release['packs']:
            name = pack['filename']
            if not re.fullmatch(r'[A-Za-z0-9_.-]+\.tar\.gz', name):
                raise ValueError('Unsafe archive name')
            path = source / name
            if path.is_symlink() or not path.is_file() or path.stat().st_size != pack['size']:
                raise ValueError('Missing or changed archive')
            if not 0 < pack['size'] <= 8 * 1024 ** 3 or not 0 < pack['unpacked_size'] <= 12 * 1024 ** 3:
                raise ValueError('Archive exceeds installer limits')
            if not re.fullmatch('[0-9a-f]{64}', pack['sha256']) or digest(path) != pack['sha256']:
                raise ValueError('Archive checksum mismatch')
            if check.get('pack_sha256', {}).get(name) != pack['sha256']:
                raise ValueError('Qualification does not cover these exact archive bytes')
            if name in archives and archives[name] != pack:
                raise ValueError('Conflicting shared runtime')
            archives[name] = pack
    return metadata, archives


def catalogs(metadata, revision):
    if not re.fullmatch('[0-9a-f]{40}', revision):
        raise ValueError('Publish to an immutable HF revision')
    catalog, routes = {'schema_version': 1, 'releases': []}, {'schema_version': 1, 'releases': []}
    for release in metadata['releases']:
        result = {key: release[key] for key in ('profile', 'os', 'arch', 'model_version', 'minimum_windows')}
        result['packs'] = [{**{key: pack[key] for key in ('kind', 'sha256', 'size', 'unpacked_size')},
                            'url': f"https://huggingface.co/{REPOSITORY}/resolve/{revision}/{pack['filename']}"}
                           for pack in release['packs']]
        catalog['releases'].append(result)
        routes['releases'].append({key: release[key] for key in ('profile', 'model_version', 'hf_revision', 'transcription', 'dictation')})
    return catalog, routes


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source', type=Path, required=True)
    parser.add_argument('--evidence', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--publish', action='store_true')
    args = parser.parse_args()
    metadata, archives = qualified_archives(args.source, json.loads(args.evidence.read_text(encoding='utf-8')))
    if not args.publish:
        print(f"Qualified {len(metadata['releases'])} profiles and {len(archives)} distinct archives; nothing uploaded.")
        return
    from huggingface_hub import CommitOperationAdd, HfApi
    api = HfApi(endpoint='https://huggingface.co')
    existing = api.repo_info(REPOSITORY, repo_type='model', files_metadata=True)
    if existing.private:
        raise ValueError('End-user downloads require the existing public model repository')
    # Reusing a published name is allowed only when the content is identical.
    remote = {item.rfilename: item for item in existing.siblings}
    operations = []
    for name, pack in sorted(archives.items()):
        item = remote.get(name)
        if item is not None:
            lfs = item.lfs
            sha = lfs.get('sha256') if isinstance(lfs, dict) else getattr(lfs, 'sha256', None)
            if item.size != pack['size'] or sha != pack['sha256']:
                raise ValueError('Published archive name has different content; use a new release name')
        else:
            operations.append(CommitOperationAdd(path_in_repo=name, path_or_fileobj=str(args.source / name)))
    commit = api.create_commit(REPOSITORY, repo_type='model', operations=operations,
                               commit_message='Add qualified Windows transcription and dictation resources') if operations else existing
    revision = commit.oid if operations else commit.sha
    published = {item.rfilename: item for item in api.repo_info(REPOSITORY, repo_type='model', revision=revision, files_metadata=True).siblings}
    for name, pack in archives.items():
        item = published[name]
        lfs = item.lfs
        sha = lfs.get('sha256') if isinstance(lfs, dict) else getattr(lfs, 'sha256', None)
        if item.size != pack['size'] or sha != pack['sha256']:
            raise ValueError('Published archive failed immutable size/checksum verification')
    catalog, routes = catalogs(metadata, revision)
    args.output.mkdir(parents=True, exist_ok=True)
    for name, data in [('asr-catalog.json', catalog), ('asr-routes.json', routes)]:
        (args.output / name).write_text(json.dumps(data, indent=2) + '\n', encoding='utf-8')
    print(f'Published and verified {len(archives)} archives at {revision}.')


if __name__ == '__main__':
    main()
