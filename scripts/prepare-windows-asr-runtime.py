#!/usr/bin/env python3
"""Maintainer-only portable Windows ASR runtime builder; never run by app setup."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess

ROOT = Path(__file__).resolve().parents[1]
PYTHON_VERSION = '3.11.11'
TORCH_VERSION = '2.9.1+cu128'
TORCH_INDEX = 'https://download.pytorch.org/whl/cu128'


def private_python(source):
    candidates = list((source / 'runtime/python').glob('*/python.exe'))
    if len(candidates) != 1:
        raise ValueError('Expected exactly one portable Windows Python interpreter')
    return candidates[0]


def validate_wheels(source):
    wheels = list((source / 'runtime/site-packages').glob('*.dist-info/WHEEL'))
    if not wheels:
        raise ValueError('Runtime contains no installed wheel metadata')
    for wheel in wheels:
        tags = [line.removeprefix('Tag: ') for line in wheel.read_text().splitlines()
                if line.startswith('Tag: ')]
        if not tags or not all(tag.endswith(('-win_amd64', '-any')) for tag in tags):
            raise ValueError(f'Not a Windows x64 wheel: {wheel.parent.name}')


def include_crt(source, crt_source, lock=None):
    lock = lock or json.loads((ROOT / 'workers/asr/windows-vc-runtime.lock.json').read_text(encoding='utf-8'))
    destination = private_python(source).parent
    for entry in lock['files']:
        name = entry['path']
        if Path(name).name != name or not name.endswith('.dll'):
            raise ValueError('Unsafe CRT DLL path')
        original = crt_source / name
        if original.is_symlink() or original.stat().st_size != entry['bytes'] or hashlib.sha256(original.read_bytes()).hexdigest() != entry['sha256']:
            raise ValueError('CRT DLL does not match its pinned SHA-256/size')
        shutil.copyfile(original, destination / name)
    notices = source / 'runtime/licenses/visual-cpp'
    notices.mkdir(parents=True, exist_ok=True)
    for name in ('NOTICE.txt', 'crt-provenance.json'):
        shutil.copyfile(crt_source / name, notices / name)
    return lock


def prepare(source, backend, uv, crt_source, python_version=PYTHON_VERSION):
    if platform.system() != 'Windows' or platform.machine().lower() not in ('amd64', 'x86_64'):
        raise ValueError('Build this private runtime on native Windows x64')
    source = source.resolve()
    runtime = source / 'runtime'
    site = runtime / 'site-packages'
    metadata = source / 'runtime-preparation.json'
    if metadata.exists():
        raise ValueError('Runtime was already prepared; use a new isolated source directory')
    runtime.mkdir(parents=True, exist_ok=True)
    cache = source.parent / 'uv-build-cache'
    env = {**os.environ, 'UV_CACHE_DIR': str(cache), 'UV_PYTHON_BIN_DIR': str(source.parent / 'python-build-links'),
           'PYTHONDONTWRITEBYTECODE': '1'}
    subprocess.run([uv, 'python', 'install', python_version, '--install-dir', str(runtime / 'python')],
                   check=True, env=env)
    python = private_python(source)
    crt = include_crt(source, crt_source)
    requirements = ROOT / f'workers/asr/requirements-windows-{backend}.lock'
    if site.exists() and any(site.iterdir()):
        raise ValueError('Private site-packages is not empty; use a fresh source directory')
    subprocess.run([uv, 'pip', 'install', '--python', str(python), '--target', str(site),
                    '--only-binary', ':all:', '--requirements', str(requirements)], check=True, env=env)
    if backend == 'cuda':
        subprocess.run([uv, 'pip', 'install', '--python', str(python), '--target', str(site),
                        '--only-binary', ':all:', '--no-deps', '--index-url', TORCH_INDEX,
                        f'torch=={TORCH_VERSION}'], check=True, env=env)
    validate_wheels(source)
    probe = '''import importlib.util,json,sys
sys.path.insert(0,sys.argv[1])
import ctypes
from pathlib import Path
crt_handles={name:ctypes.WinDLL(str(Path(sys.executable).parent/name)) for name in
 ['vcruntime140.dll','vcruntime140_1.dll','msvcp140.dll','msvcp140_1.dll']}
import numpy,scipy
info={'numpy':numpy.__version__,'scipy':scipy.__version__,'python_version':sys.version.split()[0]}
if sys.argv[2]=='onnx':
 import onnxruntime
 onnxruntime.disable_telemetry_events()
 assert importlib.util.find_spec('torch') is None
 info.update(onnxruntime=onnxruntime.__version__,providers=onnxruntime.get_available_providers(),torch_importable=False)
else:
 import torch,transformers,sentencepiece
 assert torch.__version__=='2.9.1+cu128'
 assert transformers.__version__=='4.55.4'
 info.update(torch=torch.__version__,transformers=transformers.__version__,cuda_available=torch.cuda.is_available(),cuda_arches=torch.cuda.get_arch_list())
 if torch.cuda.is_available():
  x=torch.ones(8,device='cuda');assert float(x.sum())==8
  info['gpu']=torch.cuda.get_device_name(0)
import ctypes
k=ctypes.WinDLL('kernel32',use_last_error=True)
k.GetModuleHandleW.argtypes=[ctypes.c_wchar_p];k.GetModuleHandleW.restype=ctypes.c_void_p
k.GetModuleFileNameW.argtypes=[ctypes.c_void_p,ctypes.c_wchar_p,ctypes.c_uint];k.GetModuleFileNameW.restype=ctypes.c_uint
for name,library in crt_handles.items():
 handle=library._handle;buffer=ctypes.create_unicode_buffer(32768)
 assert handle and k.GetModuleFileNameW(handle,buffer,len(buffer))
 from pathlib import Path
 assert Path(buffer.value).parent.resolve()==Path(sys.executable).parent.resolve(),name
info['app_local_crt_verified']=True
print(json.dumps(info))'''
    probe_env = {**env, 'ORT_DISABLE_TELEMETRY': '1'}
    result = subprocess.check_output([str(python), '-I', '-B', '-c', probe, str(site), backend],
                                     env=probe_env, text=True)
    info = json.loads(result)
    if backend == 'cuda' and not info['cuda_available']:
        raise ValueError('CUDA runtime failed its GPU check; do not publish this runtime')
    info.update(schema_version=1, os='windows', arch='x86_64', backend=backend,
                python=python.relative_to(source).as_posix(), site_packages='runtime/site-packages',
                requirements_sha256=hashlib.sha256(requirements.read_bytes()).hexdigest(), visual_cpp_runtime=crt)
    metadata.write_text(json.dumps(info, indent=2) + '\n', encoding='utf-8')
    print(json.dumps(info), flush=True)
    return info


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source', type=Path, required=True)
    parser.add_argument('--backend', choices=['onnx', 'cuda'], required=True)
    parser.add_argument('--uv', default='uv')
    parser.add_argument('--python-version', default=PYTHON_VERSION)
    parser.add_argument('--crt-source', type=Path, required=True, help='Output crt/ directory from prepare-windows-crt.py')
    args = parser.parse_args()
    prepare(args.source, args.backend, args.uv, args.crt_source, args.python_version)


if __name__ == '__main__':
    main()
