"""Extract pinned wheels into a separate overlay and qualify the local denoiser.

Downloads and expected digests are owned by Rust. No pip, environment mutation,
network access or user audio. A failed stage never replaces the active overlay.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import stat
import sys
import zipfile


def extract(wheels, site):
    site.mkdir(parents=True, exist_ok=False)
    total = 0
    seen = set()
    for wheel in wheels:
        with zipfile.ZipFile(wheel) as archive:
            if len(archive.infolist()) > 10000:
                raise ValueError('wheel_too_large')
            for entry in archive.infolist():
                path = PurePosixPath(entry.filename)
                if path.is_absolute() or '..' in path.parts or '\\' in entry.filename or not path.parts:
                    raise ValueError('unsafe_wheel_path')
                if stat.S_ISLNK(entry.external_attr >> 16) or entry.filename.endswith('.pth'):
                    raise ValueError('unsafe_wheel_entry')
                if entry.is_dir():
                    continue
                total += entry.file_size
                if total > 100 * 1024 * 1024 or path in seen:
                    raise ValueError('wheel_too_large_or_duplicate')
                seen.add(path)
                target = site.joinpath(*path.parts)
                target.parent.mkdir(parents=True, exist_ok=True)
                with archive.open(entry) as source, target.open('xb') as destination:
                    while block := source.read(128 * 1024):
                        destination.write(block)


def probe(root):
    import torch
    torch.set_num_threads(min(4, os.cpu_count() or 1))
    from noise_reduction import NoiseReducer
    if torch.__version__.split('+')[0] != '2.9.1' or sys.version_info[:2] != (3, 11):
        raise ValueError('noise_runtime_mismatch')
    site = root / 'site-packages'
    sys.path.insert(0, str(site))
    from inworld.voice_isolation.isolator import default_device
    device = str(default_device())
    config = {'site': str(site), 'checkpoint': str(root / 'denoiser_realtime.safetensors'),
              'device': device, 'revision': '9b0285b3fe8403aa8e2f1dbb2165a80a4294c970'}
    reducer = NoiseReducer(config)
    try:
        rtf = reducer.benchmark()
    except (RuntimeError, NotImplementedError):
        if device != 'mps':
            raise
        reducer = None
        torch.mps.empty_cache()
        config['device'] = 'cpu'
        reducer = NoiseReducer(config)
        rtf = reducer.benchmark()
    return {'device': config['device'], 'realTimeFactor': rtf, 'liveReady': rtf <= .7}


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--destination', type=Path, required=True)
    parser.add_argument('--wheels', type=Path, nargs='*', default=[])
    parser.add_argument('--probe-only', action='store_true')
    args = parser.parse_args()
    import contextlib
    import socket
    def offline(*args, **kwargs):
        raise OSError('Noise reduction setup probe is offline')
    socket.socket.connect = offline
    socket.create_connection = offline
    if os.environ.get('ETHIOPIC_MANAGED_WORKER') == '1':
        import threading
        def watch_parent():
            while os.read(0, 1):
                pass
            os._exit(1)
        threading.Thread(target=watch_parent, daemon=True).start()
    with contextlib.redirect_stdout(sys.stderr):
        if not args.probe_only:
            extract(args.wheels, args.destination / 'site-packages')
        result = probe(args.destination)
    files = []
    for path in sorted(args.destination.rglob('*')):
        if path.is_file() and path.name != 'installation.json' and '__pycache__' not in path.parts:
            with path.open('rb') as source:
                digest = hashlib.file_digest(source, 'sha256').hexdigest()
            files.append({'path': path.relative_to(args.destination).as_posix(), 'sha256': digest})
    result['files'] = files
    print(json.dumps(result, allow_nan=False), flush=True)
