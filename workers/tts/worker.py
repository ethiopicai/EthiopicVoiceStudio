"""Resident, strictly offline CPU speech synthesis; shares Studio's project/queue protocol."""
import argparse
import contextlib
import importlib.util
import json
import os
from pathlib import Path
import queue
import sys
import threading

os.environ.update(HF_HUB_OFFLINE='1',TRANSFORMERS_OFFLINE='1',HF_HUB_DISABLE_TELEMETRY='1',TOKENIZERS_PARALLELISM='false')
common=Path(__file__).resolve().parents[1]/'common'
if common.is_dir():sys.path.insert(0,str(common))
from runtime_bootstrap import prepare_private_runtime, prepare_stdin, read_stdin, ProtocolInput
prepare_private_runtime()
from engine import Projects, checked_text
from speech_generation import validate_request, write_wav_atomic

def generation_options(req):
    controls=validate_request(req,'cpu')
    return controls['temperature'],controls['sentencePause'],controls['seed']

class Provider:
    def __init__(self,root,models,manifest,threads=4,progress=lambda _:None):
        self.projects=Projects(root/'projects');self.directory=models/'tts-flow_lm-90m-amh-fp32';self.manifest=manifest
        self.threads=threads;self.progress=progress;self.tts=None;self.sentences=None
    def load(self):
        if self.tts is None:
            self.progress('Loading speech tools…')
            spec=importlib.util.spec_from_file_location('studio_small_tts',self.directory/'tts_small_infer.py')
            module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
            # FP32 on Apple Silicon: ARM int8 has not been qualified by the model team.
            self.tts=module.AmharicTTS(threads=self.threads,int8=False)
            self.sentences=module.split_sentences
        return self.tts
    def dispatch(self,req):
        if req.get('action')!='synthesize':raise ValueError('Unsupported generation action')
        temperature,pause,seed=generation_options(req)
        text=checked_text(req.get('text',''))
        if not text or len(text)>12000:raise ValueError('Use a script between 1 and 12000 characters')
        project=self.projects.get(req['projectId'])
        if project['revision']!=req['revision']:raise ValueError('Project revision changed')
        audio=self.projects.folder(project['id'])/project['versions'][project['head']]['audio']
        if audio.is_symlink() or audio.resolve().parent!=self.projects.folder(project['id']).resolve():raise ValueError('Invalid reference')
        tts=self.load();self.progress('Generating your speech…')
        import numpy as np
        import torch
        # Reference path cache is bounded across sequential jobs.
        if len(tts._voices)>=8 and str(audio) not in tts._voices:tts._voices.clear()
        # Prompt encoding also consumes random numbers. Prepare it separately so
        # a fixed generation seed behaves the same with a cold or warm voice cache.
        with torch.random.fork_rng(devices=[]):
            torch.manual_seed(0)
            tts._voice_state(str(audio))
        previous=tts.model.temp
        applied_temperature=previous if temperature is None else temperature
        # The resident worker serializes jobs. Restore model/RNG state even on failure;
        # settings must not leak into the next job or require another model load.
        try:
            tts.model.temp=applied_temperature
            with torch.random.fork_rng(devices=[]):
                torch.manual_seed(seed)
                parts=self.sentences(text) if self.sentences else [text]
                output=[];frames=0
                for sentence in parts:
                    if output:
                        silence=np.zeros(round(pause*tts.sample_rate),np.float32);output.append(silence);frames+=len(silence)
                    value=np.asarray(tts.synth(sentence,voice=str(audio)),dtype=np.float32)
                    if value.ndim!=1 or not value.size or not np.isfinite(value).all():raise ValueError('Invalid generated audio')
                    frames+=len(value)
                    if frames>tts.sample_rate*600:raise ValueError('Generated audio is too long')
                    output.append(value)
                if not output:raise ValueError('No speakable text')
                value=np.concatenate(output)
        finally:
            tts.model.temp=previous
        if value.ndim!=1 or not value.size or value.size>tts.sample_rate*600 or not np.isfinite(value).all():raise ValueError('Invalid generated audio')
        path=self.projects.folder(project['id'])/'synthesis.wav'
        write_wav_atomic(path,value,tts.sample_rate)
        variant=next(v for v in self.manifest['variants'] if v['id']=='tts-flow_lm-90m-amh-fp32')
        return {'name':'synthesis.wav','sample_rate':tts.sample_rate,'shared_tts':True,'model':'tts-flow_lm-90m-amh-fp32','model_version':variant['model_version'],'revision':self.manifest['hf_revision'],'duration':len(value)/tts.sample_rate,
                'settings':{'schemaVersion':1,'temperature':float(applied_temperature),'sentencePause':pause,'seed':seed}}

def main():
    p=argparse.ArgumentParser();p.add_argument('--root',type=Path,required=True);p.add_argument('--models',type=Path,required=True);p.add_argument('--manifest',type=Path,required=True);p.add_argument('--threads',type=int,default=4);p.add_argument('--tts');p.add_argument('--steps');p.add_argument('--device');p.add_argument('--probe',action='store_true');p.add_argument('--probe-reference',type=Path);a=p.parse_args()
    import socket
    def offline(*args,**kwargs):raise OSError('Speech synthesis is offline')
    socket.socket.connect=offline;socket.create_connection=offline
    prepare_stdin()
    if a.probe and os.environ.get('ETHIOPIC_MANAGED_WORKER')=='1':
        def watch_parent():
            try:
                while read_stdin(1):pass
            finally:os._exit(1)
        threading.Thread(target=watch_parent,daemon=True).start()
    provider=Provider(a.root,a.models,json.loads(a.manifest.read_text(encoding='utf-8')),a.threads)
    if a.probe:
        with contextlib.redirect_stdout(sys.stderr):
            import numpy as np
            tts=provider.load();out=tts.synth('ሰላም ጤና ይስጥልኝ።')
            out=np.asarray(out,dtype=np.float32)
            if out.ndim!=1 or not tts.sample_rate*.25<=len(out)<=tts.sample_rate*60 or not np.isfinite(out).all():raise ValueError('Speech probe failed')
            rms=float(np.sqrt(np.mean(out.astype(np.float64)**2)))
            if rms<1e-5:raise ValueError('Speech probe produced silence')
        print(json.dumps({'protocol':1,'probe':True,'synthesis_checked':True,'provider':'tts-flow_lm-90m-amh-fp32',
                          'sample_rate':tts.sample_rate,'generated_frames':len(out),'duration':len(out)/tts.sample_rate,'rms':rms}),flush=True);return
    output=sys.stdout;requests=queue.Queue(maxsize=8)
    def parent():
        source=ProtocolInput()
        while True:
            line=source.readline(2*1024*1024+1)
            if not line:os._exit(0)
            if len(line)>2*1024*1024 or not line.endswith(b'\n'):os._exit(1)
            try:requests.put_nowait(json.loads(line))
            except Exception:os._exit(1)
    threading.Thread(target=parent,daemon=True).start()
    print(json.dumps({'protocol':1,'ready':True}),flush=True)
    while True:
        req=requests.get();token=req.get('request')
        provider.progress=lambda stage:print(json.dumps({'protocol':1,'request':token,'stage':stage}),file=output,flush=True)
        try:
            with contextlib.redirect_stdout(sys.stderr):result=provider.dispatch(req)
            print(json.dumps({'protocol':1,'request':token,'result':result},ensure_ascii=False,allow_nan=False),flush=True)
        except Exception as exc:
            message='Speech generation could not finish. Try a shorter Amharic script and a clear reference recording.'
            if isinstance(exc,MemoryError):message='Speech generation ran out of memory. Close other applications and retry.'
            print(json.dumps({'protocol':1,'request':token,'error':{'kind':'failed','message':message}}),flush=True)
if __name__=='__main__':main()
