#!/usr/bin/env python3
"""Maintainer-only: validate and package an already prepared standalone private runtime.

No dependency installation or global Python changes. End users receive a verified archive.
The source must contain runtime/python/... and runtime/site-packages, including the editing
dependencies. A system/venv interpreter symlink outside that source is refused.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import tempfile
import tarfile
import io
import gzip


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source',type=Path,required=True);parser.add_argument('--python',required=True)
    parser.add_argument('--models',type=Path,required=True);parser.add_argument('--tts',required=True)
    parser.add_argument('--output',type=Path,required=True);parser.add_argument('--version',default='editing-2026-11-15')
    parser.add_argument('--provider',choices=['editing','small-tts'],default='editing')
    a=parser.parse_args();source=a.source.resolve();python=(source/a.python).resolve()
    if not python.is_relative_to(source) or not python.is_file():raise ValueError('Use a self-contained private interpreter')
    output=a.output.resolve()
    if output.is_relative_to(source):raise ValueError('Keep build output outside the private runtime source')
    site=source/'runtime/site-packages'
    if not site.is_dir():raise ValueError('Prepared private site-packages are missing')
    repo=Path(__file__).resolve().parents[1];small=a.provider=='small-tts';worker=repo/('workers/tts/worker.py' if small else 'workers/editing/worker.py')
    models=a.models.resolve();manifest=json.loads((models/'manifest.json').read_text(encoding='utf-8'))
    manifest.setdefault('hf_revision','local-runtime-validation')
    with tempfile.TemporaryDirectory(prefix='ethiopic-runtime-probe-') as d:
        root=Path(d);(root/'manifest.json').write_text(json.dumps(manifest))
        probe_env={**os.environ,'HF_HOME':str(root/'empty-hf'),'HF_HUB_CACHE':str(root/'empty-hf/hub'),
            'HUGGINGFACE_HUB_CACHE':str(root/'empty-hf/hub'),'TORCH_HOME':str(root/'empty-torch'),
            'XDG_CACHE_HOME':str(root/'empty-cache'),'OMNIVOICE_SRC':''}
        result=subprocess.run([str(python),'-I','-B','-u','-c',
            "import runpy,sys;sys.path.insert(0,sys.argv.pop(1));sys.path.insert(0,sys.argv.pop(1));sys.path.insert(0,sys.argv.pop(1));sys.path.insert(0,sys.argv.pop(1));runpy.run_path(sys.argv.pop(1),run_name='__main__')",
            str(site),str(worker.parent),str(repo/'workers/editing'),str(repo/'workers/common'),str(worker),'--root',str(root/'probe'),'--models',str(models),
            '--manifest',str(root/'manifest.json'),'--tts',a.tts,'--probe','--probe-reference',str(repo/'apps/desktop/ui/public/reference-voices/preview-v3/tesfaye_1_am.wav')],capture_output=True,text=True,encoding='utf-8',timeout=300,env=probe_env)
        checks=[]
        for line in result.stdout.splitlines():
            if not line.startswith('{'):continue
            try:value=json.loads(line)
            except ValueError:continue
            if isinstance(value,dict):checks.append(value)
        probe=next((value for value in checks if value.get('probe') is True and value.get('synthesis_checked') is True),None)
        if result.returncode or probe is None or (not small and (probe.get('alignment_checked') is not True or probe.get('edit_checked') is not True)):
            output.mkdir(parents=True,exist_ok=True)
            (output/'probe-stdout.log').write_text(result.stdout,encoding='utf-8')
            (output/'probe-stderr.log').write_text(result.stderr,encoding='utf-8')
            raise RuntimeError('Private runtime failed real editor/aligner load. Inspect dependencies before packaging.')
        safe_probe={key:probe[key] for key in ('protocol','probe','synthesis_checked','alignment_checked','edit_checked',
            'sample_rate','generated_frames','duration','rms','provider','shared_tts',
            'cuda_peak_allocated_bytes','cuda_peak_reserved_bytes') if key in probe}
    a.output=a.output.resolve();a.output.mkdir(parents=True,exist_ok=True)
    prepared=a.output/'prepared';prepared.mkdir(exist_ok=False)
    shutil.copytree(source/'runtime',prepared/'runtime',symlinks=True,ignore=shutil.ignore_patterns('__pycache__','*.pyc'))
    files=[]
    for path in sorted((prepared/'runtime').rglob('*')):
        if path.is_file():
            if not path.resolve().is_relative_to(prepared/'runtime'):
                raise ValueError('Runtime contains an external symlink; portable payload refused')
            with path.open('rb') as stream:digest=hashlib.file_digest(stream,'sha256').hexdigest()
            files.append({'path':path.relative_to(prepared).as_posix(),'sha256':digest})
    os_name={'Darwin':'macos','Windows':'windows','Linux':'linux'}[platform.system()]
    arch={'arm64':'aarch64','ARM64':'aarch64','AMD64':'x86_64'}.get(platform.machine(),platform.machine())
    config={'schema_version':1,'os':os_name,'arch':arch,'python':a.python,
        'site_packages':'runtime/site-packages','validated':True,'capabilities':['tts-flow_lm-90m-amh-fp32' if small else 'edit-codec_llm-0.6b-amh'],'files':files}
    # The archive extractor accepts only runtime/ payloads. The native installer
    # copies this hash-checked descriptor to the active root after extraction.
    (prepared/('runtime/tts-installation.json' if small else 'runtime/voice-edit-installation.json')).write_text(json.dumps(config,indent=2))
    entries=[]
    for path in sorted(prepared.rglob('*')):
        if path.is_file():
            with path.open('rb') as stream:digest=hashlib.file_digest(stream,'sha256').hexdigest()
            entries.append({'path':path.relative_to(prepared).as_posix(),
                            'sha256':digest,
                            'size':path.stat().st_size,'executable':path.relative_to(prepared).as_posix()==a.python or bool(path.stat().st_mode&0o111)})
    pack={'schema_version':1,'kind':'runtime','model_version':a.version,'python':a.python,
          'site_packages':'runtime/site-packages','files':entries}
    archive=a.output/f'{"tts" if small else "editing"}-runtime-{os_name}-{arch}-{a.version}.tar.gz'
    data=json.dumps(pack,sort_keys=True).encode()
    with archive.open('wb') as raw,gzip.GzipFile(fileobj=raw,mode='wb',mtime=0,compresslevel=1) as gz,tarfile.open(fileobj=gz,mode='w|') as tar:
        info=tarfile.TarInfo('pack.json');info.size=len(data);info.mode=0o644;tar.addfile(info,io.BytesIO(data))
        for entry in entries:
            path=prepared/entry['path'];info=tarfile.TarInfo(entry['path']);info.size=entry['size'];info.mode=0o755 if entry['executable'] else 0o644
            with path.open('rb') as stream:tar.addfile(info,stream)
    with archive.open('rb') as stream:archive_digest=hashlib.file_digest(stream,'sha256').hexdigest()
    result={'version':a.version,'os':os_name,'arch':arch,'python':a.python,'source':str(prepared),
            'self_test':'Offline Amharic speech generation' if small else 'SpeechEditor plus one ASR pass','publication_ready':False,
            'probe':{**safe_probe,'empty_external_caches':True},
            'artifact':{'kind':'runtime','filename':archive.name,'sha256':archive_digest,
                        'size':archive.stat().st_size,'unpacked_size':len(data)+sum(e['size'] for e in entries)},
            'next':'Qualify/sign on the target platform, publish the archive and add its pinned URL to the matching runtime catalog.'}
    (a.output/'runtime-preparation.json').write_text(json.dumps(result,indent=2)+'\n')
    print(json.dumps(result,indent=2))


if __name__=='__main__':main()
