#!/usr/bin/env python3
"""Validate a disposable copy of existing model caches; never download weights or change originals."""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--cache',type=Path,action='append',required=True)
    parser.add_argument('--cpu-runtime',type=Path)
    parser.add_argument('--gpu-runtime',type=Path)
    parser.add_argument('--output',type=Path,required=True)
    args=parser.parse_args();repo=Path(__file__).resolve().parents[1]
    manifest=json.loads((repo/'apps/desktop/resources/studio-manifest.json').read_text(encoding='utf-8'))
    previous=json.loads((repo/'apps/desktop/resources/naming-migration/manifest-v2.json').read_text(encoding='utf-8'))
    root=Path(tempfile.mkdtemp(prefix='ethiopic-naming-'));seed_models=root/'voice-edit/models'/previous['hf_revision'];seed_models.mkdir(parents=True)
    models=root/'voice-edit/models'/manifest['hf_revision']
    originals=[]
    for cache in args.cache:
        for old,current in manifest['aliases'].items():
            source=cache/old
            if not source.is_dir():source=cache/current
            target=seed_models/old
            if not source.is_dir() or target.exists():continue
            # Hard links preserve the exact weight bytes and allocate no second weight copy.
            # Rust replaces small metadata/scripts by atomic rename, leaving originals intact.
            shutil.copytree(source,target,copy_function=os.link,ignore=shutil.ignore_patterns('__pycache__','*.pyc'))
            for path in source.rglob('*'):
                if path.is_file() and path.stat().st_size>=100000:
                    info=path.stat();originals.append((path,current,path.relative_to(source),info.st_mtime_ns,info.st_size))
    (seed_models/'manifest.json').write_text(json.dumps(previous))
    env=os.environ.copy();env['ETHIOPIC_NAMING_TEST_ROOT']=str(root)
    native=subprocess.run(['cargo','test','-p','ethiopic-voice-studio','naming_migration::tests::real_local_cache_upgrade',
        '--offline','--','--ignored','--exact','--nocapture'],cwd=repo,env=env,capture_output=True,text=True,timeout=600)
    (root/'native.log').write_text(native.stdout+native.stderr,encoding='utf-8')
    if native.returncode:raise RuntimeError(f'Migration failed; inspect {root}/native.log')
    for original,current,relative,modified,size in originals:
        actual=models/current/relative
        assert original.stat().st_mtime_ns==modified and actual.stat().st_mtime_ns==modified
        assert original.stat().st_size==size and actual.stat().st_size==size
    result={'root':str(root),'manifest_version':2,'no_weight_downloads':True,'original_weights_unchanged':True,
            'checked_weight_files':len(originals),'migrated_models':[v['id'] for v in manifest['variants'] if (models/v['id']).is_dir()]}
    env.pop('ETHIOPIC_MANAGED_WORKER',None)
    env.update(HF_HOME=str(root/'empty-hf'),HF_HUB_OFFLINE='1',TRANSFORMERS_OFFLINE='1',PYTHONDONTWRITEBYTECODE='1')
    for kind,runtime in [('gpu',args.gpu_runtime),('cpu',args.cpu_runtime)]:
        if runtime is None:continue
        descriptor=runtime/'installation.json'
        if not descriptor.exists():descriptor=runtime/'runtime'/('voice-edit-installation.json' if kind=='gpu' else 'tts-installation.json')
        config=json.loads(descriptor.read_text(encoding='utf-8'))
        py=runtime/config['python'];site=runtime/config['site_packages'];worker=repo/'workers'/('editing' if kind=='gpu' else 'tts')/'worker.py'
        command=[str(py),'-I','-B','-u','-c',
            "import runpy,sys;sys.path.insert(0,sys.argv.pop(1));sys.path.insert(0,sys.argv.pop(1));runpy.run_path(sys.argv.pop(1),run_name='__main__')",
            str(site),str(repo/'workers/editing'),str(worker),'--root',str(root/kind),'--models',str(models),
            '--manifest',str(models/'manifest.json'),'--probe']
        if kind=='gpu':command+=['--tts',manifest['bundles']['gpu-modern']['tts'],'--steps','32','--device','cuda:0',
            '--probe-reference',str(repo/'apps/desktop/ui/public/reference-voices/preview-v3/tesfaye_1_am.wav')]
        job=subprocess.run(command,input='',capture_output=True,text=True,encoding='utf-8',env=env,timeout=900)
        (root/f'{kind}.log').write_text(job.stdout+'\n'+job.stderr,encoding='utf-8')
        messages=[]
        for line in job.stdout.splitlines():
            try:messages.append(json.loads(line))
            except ValueError:pass
        probe=next((v for v in messages if v.get('probe')),None)
        result[kind]={'passed':job.returncode==0 and probe is not None,'probe':probe}
    args.output.parent.mkdir(parents=True,exist_ok=True);args.output.write_text(json.dumps(result,indent=2)+'\n',encoding='utf-8')
    print(json.dumps(result),flush=True)
    if any(not result[kind]['passed'] for kind in ['gpu','cpu'] if kind in result):raise SystemExit(1)

if __name__=='__main__':main()
