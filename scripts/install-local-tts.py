#!/usr/bin/env python3
"""Maintainer-only local preview activation. End users use Studio's resource installer."""
import argparse,fcntl,hashlib,json,shutil,tempfile
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('--prepared',type=Path,required=True);p.add_argument('--models',type=Path,required=True);p.add_argument('--app-data',type=Path,required=True);a=p.parse_args()
root=a.app_data.resolve();root.mkdir(parents=True,exist_ok=True)
with (root/'recognition.lock').open('a+b') as lock:
 fcntl.flock(lock,fcntl.LOCK_EX|fcntl.LOCK_NB)
 manifest=json.loads((a.models/'manifest.json').read_text());variant=next(v for v in manifest['variants'] if v['id']=='tts-flow_lm-90m-amh-fp32')
 for item in variant['files']:
  file=a.models/variant['folder']/item['path']
  with file.open('rb') as stream:assert hashlib.file_digest(stream,'sha256').hexdigest()==item['sha256']
 config=json.loads((a.prepared/'runtime/tts-installation.json').read_text());assert config['validated'] and config['capabilities']==['tts-flow_lm-90m-amh-fp32']
 for item in config['files']:
  with (a.prepared/item['path']).open('rb') as stream:assert hashlib.file_digest(stream,'sha256').hexdigest()==item['sha256']
 base=root/'tts-runtime';base.mkdir(exist_ok=True)
 stage=Path(tempfile.mkdtemp(prefix='staging-',dir=base));shutil.copytree(a.prepared/'runtime',stage/'runtime',symlinks=True)
 (stage/'installation.json').write_text(json.dumps(config,indent=2)+'\n')
 # No live Studio worker may retain this runtime while its directory moves.
 voice=root/'voice-edit';voice.mkdir(exist_ok=True)
 with (voice/'voice-worker.lock').open('a+b') as worker:
  fcntl.flock(worker,fcntl.LOCK_EX|fcntl.LOCK_NB)
  active=base/'active';previous=base/'previous'
  if previous.exists():raise RuntimeError('A previous runtime is retained. Review it before another local activation.')
  if active.exists():active.rename(previous)
  stage.rename(active)
 target=voice/'models'/manifest['hf_revision'];target.mkdir(parents=True,exist_ok=True)
 for item in variant['files']:
  source=a.models/variant['folder']/item['path'];destination=target/variant['folder']/item['path'];destination.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(source,destination)
 (target/'manifest.json').write_text(json.dumps(manifest))
 (target/'tts-verified.json').write_text(json.dumps({'revision':manifest['hf_revision'],'tts':'tts-flow_lm-90m-amh-fp32'}))
print('Activated private speech runtime and pinned HF model:',root)
