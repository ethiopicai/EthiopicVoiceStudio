#!/usr/bin/env python3
"""Reject retired model IDs outside the retained migration/alias metadata."""
import json
from pathlib import Path

ROOT=Path(__file__).resolve().parents[1]
MANIFEST=ROOT/'apps/desktop/resources/studio-manifest.json'
ALIASES=json.loads(MANIFEST.read_text(encoding='utf-8'))['aliases']

def current_metadata(value):
    if isinstance(value,dict):return {k:current_metadata(v) for k,v in value.items() if k not in ('aliases','previous_name')}
    if isinstance(value,list):return [current_metadata(v) for v in value]
    return value

def check():
    failures=[]
    files=[ROOT/'README.md',ROOT/'THIRD_PARTY_NOTICES.md']
    for name in ['apps','crates','workers','scripts']:
        files.extend(p for p in (ROOT/name).rglob('*') if p.is_file() and p.suffix in {'.rs','.py','.json','.js','.ts','.mjs','.svelte','.md','.txt'}
                     and not any(part in {'node_modules','dist','target','__pycache__','naming-migration'} for part in p.parts))
    for path in files:
        text=path.read_text(encoding='utf-8')
        if path==MANIFEST or path==ROOT/'apps/desktop/resources/release-catalog.json':
            text=json.dumps(current_metadata(json.loads(text)))
        for index,line in enumerate(text.splitlines(),1):
            if any(old in line for old in ALIASES):failures.append(f'{path.relative_to(ROOT)}:{index}')
    if failures:raise SystemExit('Retired model IDs remain outside migration metadata:\n'+'\n'.join(failures))
    print(f'PASS: {len(ALIASES)} retired IDs occur only in retained alias/migration metadata.')

if __name__=='__main__':check()
