#!/usr/bin/env python3
"""Keep build-machine paths out of distributable executables."""
import argparse
import os
from pathlib import Path


def build_paths(root, env):
    home = Path(env.get('USERPROFILE') or env.get('HOME') or str(Path.home()))
    cargo = Path(env.get('CARGO_HOME') or home / '.cargo')
    paths = []
    for source, replacement in [(home, '/build/user'), (Path(root), '/studio/src'), (cargo, '/rust/cargo')]:
        for value in [source.absolute(), source.resolve()]:
            if (str(value), replacement) not in paths:
                paths.append((str(value), replacement))
    return paths


def build_environment(root, environment=None):
    env = dict(os.environ if environment is None else environment)
    existing = env.get('CARGO_ENCODED_RUSTFLAGS')
    flags = existing.split('\x1f') if existing else env.get('RUSTFLAGS', '').split()
    # More specific mappings come last, as required by rustc.
    for source, replacement in build_paths(root, env):
        flags.append(f'--remap-path-prefix={source}={replacement}')
    env['CARGO_ENCODED_RUSTFLAGS'] = '\x1f'.join(flags)
    return env


def reject_build_paths(binary, root, environment=None):
    env = os.environ if environment is None else environment
    data = Path(binary).read_bytes()
    for source, _ in build_paths(root, env):
        for value in {source, source.replace('\\', '/'), source.replace('/', '\\')}:
            for encoding in ['utf-8', 'utf-16-le']:
                if value.encode(encoding) in data:
                    raise ValueError('Executable contains a build-machine path; enable compiler path remapping before distribution')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--github-env', type=Path, required=True)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    value = build_environment(root)['CARGO_ENCODED_RUSTFLAGS']
    with args.github_env.open('a', encoding='utf-8') as stream:
        stream.write('CARGO_ENCODED_RUSTFLAGS=' + value + '\n')


if __name__ == '__main__':
    main()
