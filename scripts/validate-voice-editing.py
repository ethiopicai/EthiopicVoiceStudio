#!/usr/bin/env python3
"""Maintainer end-to-end tests against the actual resident worker on the model source host."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import time


def main():
    parser=argparse.ArgumentParser();parser.add_argument('--models',type=Path,required=True)
    parser.add_argument('--worker',type=Path,required=True);parser.add_argument('--task',type=Path,required=True)
    parser.add_argument('--output',type=Path,required=True);parser.add_argument('--cpu',action='store_true')
    a=parser.parse_args();a.output.mkdir(parents=True,exist_ok=True)
    sys.path.insert(0,str(a.worker.parent))
    from audio import PCM,preserved
    task=json.loads(a.task.read_text());original=Path(task['path']);digest=hashlib.sha256(original.read_bytes()).hexdigest()
    # Exercise a spoken-word change together with punctuation outside its audio span.
    task['new_text']=task['new_text'].rstrip('።.!?')+'!'
    import soundfile as sf
    values,rate=sf.read(original,dtype='float32',always_2d=True)
    fixture=PCM(rate,values.shape[1],2,b'')
    # Decode the owned compressed fixture once. The app accepts integer PCM WAV only.
    import numpy as np
    data=np.rint(np.clip(values,-1,1)*32768).clip(-32768,32767).astype('<i2').tobytes()
    fixture=PCM(rate,values.shape[1],2,data)
    source=a.output/'input.wav';fixture.write(source)
    manifest=json.loads((a.models/'manifest.json').read_text());manifest['hf_revision']='source-reference-validation'
    (a.output/'manifest.json').write_text(json.dumps(manifest))
    cmd=[sys.executable,'-B','-u',str(a.worker),'--root',str(a.output),'--models',str(a.models),
         '--manifest',str(a.output/'manifest.json'),'--tts','tts-codec_llm-0.6b-amh-int8_onnx' if a.cpu else 'tts-codec_llm-0.6b-amh-fp16',
         '--steps','16' if a.cpu else '32','--threads','6' if a.cpu else '4','--device','cpu' if a.cpu else 'cuda:0']
    child=subprocess.Popen(cmd,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=(a.output/'worker.log').open('w'),text=True)
    model_worker_pid=child.pid;number=0;latencies={}
    def request(action,**fields):
        nonlocal number
        number+=1;started=time.perf_counter()
        child.stdin.write(json.dumps({'request':number,'action':action,**fields},ensure_ascii=False)+'\n');child.stdin.flush()
        while True:
            line=child.stdout.readline()
            if not line:raise RuntimeError('Worker stopped; inspect its private test log')
            message=json.loads(line)
            if message.get('request')!=number:continue
            if 'error' in message:raise RuntimeError(json.dumps(message['error']))
            if 'result' in message:
                latencies.setdefault(action,[]).append(round(time.perf_counter()-started,3))
                return message['result']
    try:
        p=request('create',source=str(source),name='Held-out validation',text=task['orig_text'])
        p=request('align',projectId=p['id'],revision=p['revision'],text=task['orig_text'])
        preview=request('preview',projectId=p['id'],revision=p['revision'],text=task['new_text'],seed=41)
        before=PCM.read(a.output/'projects'/p['id']/'original.wav')
        after=PCM.read(a.output/'projects'/p['id']/'preview.wav')
        assert preserved(before,after,preview['edits'])
        p=request('keep',projectId=p['id'],revision=p['revision'])
        log=request('export',projectId=p['id'],revision=p['revision'])
        assert log['synthetic'] and log['regenerated_regions'] and log['history'][-1]['seed']==41
        # Chain a second real edit on the kept audio, preserving its permanent synthetic marks.
        kept=p;kept_pcm=PCM.read(a.output/'projects'/p['id']/p['versions'][p['head']]['audio'])
        chained=request('preview',projectId=p['id'],revision=p['revision'],text=task['orig_text'],seed=43)
        chained_pcm=PCM.read(a.output/'projects'/p['id']/'preview.wav')
        assert preserved(kept_pcm,chained_pcm,chained['edits'])
        for mark in chained['regions']:
            assert mark['frames_out']==[round(t*chained_pcm.rate) for t in mark['span_out']]
        p=request('keep',projectId=p['id'],revision=p['revision'])
        chain_log=request('export',projectId=p['id'],revision=p['revision'])
        assert [v['seed'] for v in chain_log['history'] if v['kind']=='edit']==[41,43]
        p=request('undo',projectId=p['id'],revision=p['revision'])
        assert p['head']==kept['head']
        p=request('undo',projectId=p['id'],revision=p['revision'])
        assert p['versions'][p['head']]['text']==task['orig_text']
        retry=request('preview',projectId=p['id'],revision=p['revision'],text=task['new_text'],seed=42)
        assert retry['seed']==42
        synthesis=request('synthesize',projectId=p['id'],revision=p['revision'],text='እሺ።',referenceText=task['orig_text'],seed=3)
        assert synthesis['shared_tts'] and (a.output/'projects'/p['id']/'synthesis.wav').is_file()
        long_result=None
        if not a.cpu:
            # A >30-second WAV assembled from an owned held-out recording, with known source transcript.
            silence=b'\0'*(round(before.rate*.7)*before.stride)
            long_audio=PCM(before.rate,before.channels,before.width,(before.data+silence)*5)
            path=a.output/'long-input.wav';long_audio.write(path)
            text=' '.join([task['orig_text']]*5)
            words=p['versions'][p['head']]['words'];anchors=[]
            for repeat in range(5):
                offset=repeat*(before.duration+.7)
                anchors.extend(dict(w,start=w['start']+offset,end=w['end']+offset) for w in words)
            lp=request('create',source=str(path),name='Long recording validation',text=text,words=anchors)
            lp=request('align',projectId=lp['id'],revision=lp['revision'],text=text)
            targets=[task['new_text'],task['orig_text'],task['orig_text'],task['orig_text'],task['new_text']]
            out=request('preview',projectId=lp['id'],revision=lp['revision'],text=' '.join(targets),seed=71)
            edited=PCM.read(a.output/'projects'/lp['id']/'preview.wav')
            assert preserved(long_audio,edited,out['edits'])
            assert len(out['edits'])>=2
            lp=request('keep',projectId=lp['id'],revision=lp['revision'])
            exported=request('export',projectId=lp['id'],revision=lp['revision'])
            assert exported['words']==lp['versions'][lp['head']]['words']
            long_result={'input_seconds':long_audio.duration,'output_seconds':edited.duration,
                         'regions':len(out['edits']),'untouched_pcm_identical':True,
                         'construction':'five held-out source clips with 0.7-second pauses'}
        # Interrupt a real generation at the same process boundary as the native Stop action.
        # A stale preview file is never a committed take, and restart must recover the saved head.
        project_file=a.output/'projects'/p['id']/'project.json';saved_head=project_file.read_bytes()
        number+=1
        child.stdin.write(json.dumps({'request':number,'action':'preview','projectId':p['id'],
            'revision':p['revision'],'text':task['new_text'],'seed':99},ensure_ascii=False)+'\n');child.stdin.flush()
        while True:
            line=child.stdout.readline()
            if not line:raise RuntimeError('Worker stopped before interruption test')
            event=json.loads(line)
            if event.get('request')!=number:continue
            if event.get('stage','').startswith('Regenerating change'):
                child.kill();child.wait(timeout=15);break
            if 'error' in event or 'result' in event:raise RuntimeError('No generation stage received for interruption test')
        child.stdin.close()
        assert project_file.read_bytes()==saved_head
        child=subprocess.Popen(cmd,stdin=subprocess.PIPE,stdout=subprocess.PIPE,
            stderr=(a.output/'worker-restart.log').open('w'),text=True)
        recovered=request('open',projectId=p['id'])
        assert recovered['head']==p['head'] and recovered['revision']==p['revision']
        assert hashlib.sha256(original.read_bytes()).hexdigest()==digest
        report={'tier':'cpu-high opt-in' if a.cpu else 'gpu-modern','device':'cpu' if a.cpu else 'cuda:0',
                'num_step':16 if a.cpu else 32,'threads':6 if a.cpu else 4,'worker_pid':model_worker_pid,
                'original_unchanged':True,'untouched_pcm_identical':True,'undo_export_passed':True,
                'retry_new_seed_passed':True,'shared_tts_synthesis_passed':True,'latency_seconds':latencies,
                'chained_edit_and_marker_shift_passed':True,
                'generation_interruption_preserved_saved_head':True,'restart_reopened_saved_head':True,
                'long_recording':long_result}
        (a.output/'validation.json').write_text(json.dumps(report,indent=2)+'\n')
        print(json.dumps(report,indent=2),flush=True)
    finally:
        child.stdin.close()
        try:child.wait(timeout=20)
        except subprocess.TimeoutExpired:child.kill();child.wait()


if __name__=='__main__':main()
