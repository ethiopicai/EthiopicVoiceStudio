#!/usr/bin/env python3
"""Real, offline Mac CPU provider validation using the exact resident-worker protocol."""
import argparse,hashlib,json,os,shutil,subprocess,sys,time,wave
from pathlib import Path
p=argparse.ArgumentParser();p.add_argument('--runtime',type=Path,required=True);p.add_argument('--models',type=Path,required=True);p.add_argument('--output',type=Path,required=True);p.add_argument('--advanced',action='store_true');a=p.parse_args()
repo=Path(__file__).resolve().parents[1];runtime=a.runtime.resolve();models=a.models.resolve();out=a.output.resolve();out.mkdir(parents=True,exist_ok=True)
sys.path.insert(0,str(repo/'workers/editing'));from engine import Projects
projects=Projects(out/'projects');source=models/'tts-flow_lm-90m-amh-fp32/voices/default.wav';original=source.read_bytes()
py=runtime/'runtime/python/cpython-3.11.12-macos-aarch64-none/bin/python3';site=runtime/'runtime/site-packages';worker=repo/'workers/tts/worker.py'
cmd=[str(py),'-I','-B','-u','-c',"import runpy,sys;sys.path.insert(0,sys.argv.pop(1));sys.path.insert(0,sys.argv.pop(1));runpy.run_path(sys.argv.pop(1),run_name='__main__')",str(site),str(repo/'workers/editing'),str(worker),'--root',str(out),'--models',str(models),'--manifest',str(models/'manifest.json')]
child=subprocess.Popen(cmd,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=(out/'worker-stderr.log').open('w'),text=True,env={**os.environ,'HF_HUB_OFFLINE':'1','HF_HUB_DISABLE_TELEMETRY':'1'})
receipts=[]
def request(project,text,token,revision=None,settings=None,seed=0):
 before=(out/'projects'/project['id']/'original.wav').read_bytes();started=time.monotonic()
 child.stdin.write(json.dumps({'action':'synthesize','projectId':project['id'],'revision':project['revision'] if revision is None else revision,'text':text,'request':token,'referenceText':'','seed':seed,'generationOptions':settings})+'\n');child.stdin.flush()
 while True:
  line=child.stdout.readline()
  if not line:raise RuntimeError('worker stopped')
  value=json.loads(line)
  if value.get('request')==token and ('result'in value or 'error'in value):break
 assert before==(out/'projects'/project['id']/'original.wav').read_bytes()
 return value,time.monotonic()-started
try:
 assert json.loads(child.stdout.readline())['ready']
 texts=['ሰላም ጤና ይስጥልኝ።','እንዴት ናችሁ። ዛሬ ስለ ኢትዮጵያ ታሪክ እንነጋገራለን።']
 for i,text in enumerate(texts):
  reference=source if i==0 else out/'projects'/receipts[0]['projectId']/'synthesis.wav'
  project=projects.create(reference,'Default' if i==0 else 'Cloned reference')
  result,elapsed=request(project,text,i+1);assert 'result'in result,result
  metadata=result['result'];path=out/'projects'/project['id']/'synthesis.wav'
  with wave.open(str(path)) as wav:
   assert wav.getframerate()==24000 and wav.getnchannels()==1 and wav.getsampwidth()==2
   duration=wav.getnframes()/24000;assert duration>0
  assert metadata['model']=='tts-flow_lm-90m-amh-fp32' and metadata['model_version']
  retained=out/('default.wav' if i==0 else 'clone.wav');shutil.copyfile(path,retained)
  receipts.append({'audioFile':retained.name,'projectId':project['id'],'kind':'default' if i==0 else 'clone','elapsedSeconds':round(elapsed,3),'audioSeconds':duration,'rtf':round(elapsed/duration,3),'modelVersion':metadata['model_version'],'sha256':hashlib.sha256(path.read_bytes()).hexdigest(),'workerPid':child.pid})
  print(json.dumps(receipts[-1]),flush=True)
 advanced=[]
 if a.advanced:
  project=projects.create(source,'Advanced controls');script='ሰላም ጤና ይስጥልኝ። ዛሬ እንዴት ናችሁ።'
  for token,(label,temperature,pause,seed) in enumerate([('baseline',.3,0,47),('repeat',.3,0,47),('new-seed',.3,0,48),('temperature',.6,0,47),('pause',.3,.5,47)],20):
   value,elapsed=request(project,script,token,settings={'temperature':temperature,'sentencePause':pause},seed=seed);assert 'result'in value,value
   path=out/'projects'/project['id']/'synthesis.wav';retained=out/f'advanced-{label}.wav';shutil.copyfile(path,retained)
   with wave.open(str(path)) as wav:frames=wav.getnframes()
   settings=value['result']['settings'];assert settings=={'schemaVersion':1,'temperature':temperature,'sentencePause':pause,'seed':seed}
   advanced.append({'kind':label,'settings':settings,'frames':frames,'sha256':hashlib.sha256(path.read_bytes()).hexdigest(),'elapsedSeconds':round(elapsed,3),'workerPid':child.pid})
  # The threaded codec batches queued frames dynamically. Quantized PCM may differ
  # by one least-significant bit even when seeded latent generation is identical.
  import numpy as np
  def samples(label):
   with wave.open(str(out/f'advanced-{label}.wav')) as wav:return np.frombuffer(wav.readframes(wav.getnframes()),dtype='<i2').astype(np.int32)
  baseline,repeat=samples('baseline'),samples('repeat');assert baseline.shape==repeat.shape
  same_seed_max_pcm_difference=int(np.max(np.abs(baseline-repeat)))
  assert same_seed_max_pcm_difference<=1,'same seed/settings must repeat within one PCM16 unit on this CPU/runtime'
  advanced[1]['maxPcmDifferenceFromBaseline']=same_seed_max_pcm_difference
  assert advanced[0]['sha256']!=advanced[2]['sha256'],'different seeds must affect the waveform'
  assert advanced[0]['sha256']!=advanced[3]['sha256'],'temperature must affect the waveform'
  assert advanced[4]['frames']-advanced[0]['frames']==12000,'half-second pause must add exactly 12000 frames'
  value,_=request(project,script,30);assert value['result']['settings']['temperature']==.3,'per-job temperature must restore'
  print(json.dumps({'advancedControls':advanced}),flush=True)
 result,_=request(project,'ሰላም።',10,revision=999);assert 'error'in result
 result,_=request(project,'ሰላም።',11);assert 'result'in result,'worker recovers after a rejected request'
 assert source.read_bytes()==original
 # Interrupt by exact child PID, then confirm immutable inputs/metadata remain readable.
 before=(out/'projects'/project['id']/'project.json').read_bytes()
 child.stdin.write(json.dumps({'action':'synthesize','projectId':project['id'],'revision':0,'text':'ሰላም ጤና ይስጥልኝ። '*100,'request':12})+'\n');child.stdin.flush()
 time.sleep(.2);child.terminate();child.wait(timeout=10)
 assert (out/'projects'/project['id']/'project.json').read_bytes()==before
 assert projects.get(project['id'])['revision']==0
 receipt={'offline':True,'precision':'fp32','device':'cpu','modelSource':'pinned HF manifest files','jobs':receipts,'advancedControls':advanced,'sameResidentWorker':len({r['workerPid'] for r in receipts+advanced})==1,'staleRevisionRejected':True,'recoveryAfterRejectedRequest':True,'cancelOriginalPreserved':True}
 (out/'validation.json').write_text(json.dumps(receipt,indent=2)+'\n');print('PASS',flush=True)
finally:
 if child.poll() is None:child.terminate();child.wait(timeout=10)
