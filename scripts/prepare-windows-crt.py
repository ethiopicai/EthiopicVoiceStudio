#!/usr/bin/env python3
"""Extract pinned Microsoft app-local CRT files; never installs system components."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import struct
import subprocess
import urllib.request
import xml.etree.ElementTree as ET

LOCK = Path(__file__).resolve().parents[1] / 'workers/asr/windows-vc-runtime.lock.json'


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def extract(installer, output, sevenzip, lock):
    if digest(installer) != lock['sha256']:
        raise ValueError('Microsoft redistributable does not match its pinned SHA-256')
    output.mkdir(parents=True, exist_ok=True)
    data = installer.read_bytes()
    offset, cabinets = 0, []
    while True:
        offset = data.find(b'MSCF', offset)
        if offset < 0:
            break
        if offset + 36 > len(data):
            break
        size = struct.unpack_from('<I', data, offset + 8)[0]
        if size >= 36 and offset + size <= len(data) and data[offset + 4:offset + 8] == b'\0' * 4:
            target = output / f'container-{len(cabinets)}'
            cabinet = target.with_suffix('.cab')
            cabinet.write_bytes(data[offset:offset + size])
            subprocess.run([sevenzip, 'x', '-y', str(cabinet), '-o' + str(target)], check=True)
            cabinets.append(target)
        offset += 4
    if len(cabinets) != 2:
        raise ValueError('Unexpected Microsoft redistributable container structure')
    manifest = ET.parse(cabinets[0] / '0')
    payloads = [p for p in manifest.getroot() if p.tag.endswith('Payload')
                and p.get('FilePath', '').endswith('vcRuntimeMinimum_amd64\\cab1.cab')]
    if len(payloads) != 1 or not payloads[0].get('SourcePath', '').isalnum():
        raise ValueError('Pinned x64 CRT payload is missing')
    dlls = output / 'dlls'
    subprocess.run([sevenzip, 'x', '-y', str(cabinets[1] / payloads[0].get('SourcePath')),
                    '-o' + str(dlls)], check=True)
    final = output / 'crt'
    final.mkdir(exist_ok=True)
    for entry in lock['files']:
        original = dlls / entry['source']
        if original.stat().st_size != entry['bytes'] or digest(original) != entry['sha256']:
            raise ValueError('Extracted CRT DLL failed its pinned SHA-256/size check')
        shutil.copyfile(original, final / entry['path'])
    (final / 'crt-provenance.json').write_text(json.dumps(lock, indent=2) + '\n', encoding='utf-8')
    (final / 'NOTICE.txt').write_text(
        f"Microsoft Visual C++ Runtime {lock['version']} (x64).\n"
        'Copyright Microsoft Corporation. All rights reserved.\n'
        'Redistributed under the applicable Microsoft Visual Studio license terms:\n'
        f"{lock['license_url']}\nSource: {lock['url']}\n"
        f"Redistributable SHA-256: {lock['sha256']}\n", encoding='utf-8')
    return final


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--installer', type=Path)
    parser.add_argument('--sevenzip', default='C:\\Program Files\\7-Zip\\7z.exe')
    args = parser.parse_args()
    lock = json.loads(LOCK.read_text(encoding='utf-8'))
    installer = args.installer
    if installer is None:
        args.output.mkdir(parents=True, exist_ok=True)
        installer = args.output / 'vc_redist.x64.exe'
        if not installer.exists() or digest(installer) != lock['sha256']:
            urllib.request.urlretrieve(lock['url'], installer)
    print(extract(installer, args.output, args.sevenzip, lock))


if __name__ == '__main__':
    main()
