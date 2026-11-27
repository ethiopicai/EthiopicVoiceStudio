#!/usr/bin/env python3
"""Maintainer-only runtime builder. Not part of the end-user app/setup flow."""
from pathlib import Path
import platform
import subprocess


def main():
    if platform.system() != 'Darwin' or platform.machine() != 'arm64':
        raise SystemExit('This validated runtime recipe targets macOS arm64 only. Build/test a separate pack for other platforms.')
    runtime = Path('.local-asr/runtime')
    subprocess.run(['uv', 'python', 'install', '3.11.12', '--install-dir', str(runtime / 'python')], check=True)
    python = runtime / 'python/cpython-3.11.12-macos-aarch64-none/bin/python3'
    subprocess.run(['uv', 'pip', 'install', '--python', str(python), '--target', str(runtime / 'site-packages'),
                    '--requirements', 'workers/asr/requirements-macos-arm64.lock'], check=True)
    print('Private development runtime prepared. Activate it with install-local-asr.py; do not bundle it into the main app.')


if __name__ == '__main__':
    main()
