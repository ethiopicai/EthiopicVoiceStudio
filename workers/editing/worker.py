"""Resident offline voice provider. One serialized queue owns editor and shared TTS."""
import argparse
import contextlib
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import queue
import sys
import threading
import time

os.environ.update(ORT_DISABLE_TELEMETRY='1', HF_HUB_OFFLINE='1', TRANSFORMERS_OFFLINE='1',
                  HF_HUB_DISABLE_TELEMETRY='1', TOKENIZERS_PARALLELISM='false', PYTHONDONTWRITEBYTECODE='1')

common=Path(__file__).resolve().parents[1]/'common'
if common.is_dir():sys.path.insert(0,str(common))
from runtime_bootstrap import prepare_private_runtime, prepare_stdin, read_stdin, ProtocolInput
prepare_private_runtime()
from audio import PCM
from engine import Projects, Refused, ERRORS, checked_text, align_project, change_audio, shift_regions
from speech_generation import validate_request, synthesize_omnivoice, write_wav_atomic


def module(path,name):
    spec=importlib.util.spec_from_file_location(name,path)
    value=importlib.util.module_from_spec(spec);spec.loader.exec_module(value);return value


class Provider:
    def __init__(self,root,models,manifest,tts,steps,threads=4,device=None,progress=lambda _:None):
        self.projects=Projects(root/'projects');self.root=root;self.models=models;self.manifest=manifest
        self.tts_id=tts;self.steps=steps;self.threads=threads;self.device=device
        self.progress=progress;self.editor=None;self.preview=None;self.voices={}
        aligner=next(v for v in manifest['variants'] if v['id']=='asr-wav2vec_ctc-1b-amh-int8_onnx')
        self.model={"editor":"edit-codec_llm-0.6b-amh","tts":tts,"revision":manifest['hf_revision'],
                    "aligner":aligner['id'],"aligner_model":aligner.get('model','amharic-ctc'),
                    "aligner_version":aligner['model_version']}

    def load(self):
        if self.editor is not None:return self.editor
        self.progress('Loading voice tools…')
        # Dependencies are checked/installed by the native parent. No package installation or network here.
        import onnxruntime as ort
        ort.disable_telemetry_events()
        directory=self.models/'edit-codec_llm-0.6b-amh'
        sys.path.insert(0,str(directory))
        mod=module(directory/'edit_omnivoice_infer.py','studio_speech_editor')
        load_module=mod._load_module
        variant=next(v for v in self.manifest['variants'] if v['id']==self.tts_id)
        def with_metadata(path,name):
            value=load_module(path,name)
            if name=='_edit_tts_omnivoice_infer':
                # The owner manifest omits variant.json. Supply its validated metadata in memory,
                # preserving every downloaded file and avoiding incorrect fp32/CPU fallback.
                value._variant_meta=lambda _:variant
            return value
        mod._load_module=with_metadata
        self.editor=mod.SpeechEditor(str(self.models/self.tts_id),
            codec_dir=str(self.models/'codec-higgs_audio_v2-fp32'),aligner_dir=str(self.models/'asr-wav2vec_ctc-1b-amh-int8_onnx'),
            device=self.device,threads=self.threads,num_step=self.steps)
        self.editor._studio_norm=mod.norm_word
        self.editor.aligner.asr.version=self.model['aligner_version']
        return self.editor

    def dispatch(self,req):
        action=req['action'];identifier=req.get('projectId');revision=req.get('revision')
        if action=='synthesize':
            controls=validate_request(req,'omnivoice',self.steps)
            text=checked_text(req.get('text',''));reference=checked_text(req.get('referenceText',''))
            if not text or not reference:raise Refused('empty_text')
            if len(text)>12000:raise ValueError('Use a script up to 12000 characters')
        if action=='list':return self.projects.list()
        if action=='create':
            project=self.projects.create(req['source'],req['name'],req.get('text',''),req.get('words'))
            project['consent']=True;self.projects.save(project)
            return project
        if action=='open':return self.projects.get(identifier)
        if action=='undo':
            self.preview=None
            return self.projects.undo(identifier,revision)
        if action=='export':return self.projects.export_log(identifier)
        if action=='release':self.editor=None;self.preview=None;self.voices={};return {'released':True}
        project=self.projects.get(identifier)
        if project['revision']!=revision:raise ValueError('Project changed. Reopen it before continuing.')
        current=project['versions'][project['head']]
        if action=='keep':
            preview=self.preview
            if not preview or preview['id']!=identifier or preview['revision']!=revision:
                raise ValueError('This preview is out of date. Regenerate before keeping it.')
            # Preview WAV/metadata are immutable until explicitly kept or replaced.
            p=self.projects.commit(identifier,revision,preview['text'],preview['words'],
                audio=PCM.read(self.projects.folder(identifier)/'preview.wav'),regions=preview['regions'],
                kind='edit',seed=preview['seed'],model=self.model)
            self.preview=None;return p
        pcm=self.projects.audio(project);ed=self.load()
        if action in ('align','fix'):
            self.progress('Aligning words with your recording…')
            text=checked_text(req.get('text',current['text']))
            if not text:
                result=ed.transcribe(pcm.mono(),sr=pcm.rate);text=result['text'];coarse=result['words']
            else:coarse=current['words'] if text==current['text'] else None
            if not text:raise Refused('empty_text')
            words=align_project(ed,pcm,text,coarse)
            self.preview=None
            return self.projects.commit(identifier,revision,text,words,kind='fix' if action=='fix' else 'align',model=self.model)
        if action=='preview':
            started=time.perf_counter()
            audio,words,marks=change_audio(ed,pcm,current['text'],req.get('text',''),current['words'],
                req['seed'],self.steps,self.progress)
            if len(audio.data)>100*1024*1024:raise ValueError('Edited recording exceeds 100 MB')
            existing=shift_regions(current['regions'],marks,pcm.rate)
            for mark in marks:mark.update(seed=req['seed'],model=self.model,synthetic=True)
            self.preview={'id':identifier,'revision':revision,'text':checked_text(req['text']),
                          'words':words,'regions':existing+marks,'edits':marks,'seed':req['seed'],
                          'duration':audio.duration,'latency_sec':round(time.perf_counter()-started,3)}
            audio.write(self.projects.folder(identifier)/'preview.wav')
            return self.preview
        if action=='synthesize':
            # Normal OmniVoice synthesis uses this same resident editor's TTS instance and queue.
            ref=self.projects.folder(identifier)/current['audio']
            key=hashlib.sha256(ref.read_bytes()+reference.encode()).hexdigest()
            if key not in self.voices:
                if len(self.voices)>=16:self.voices.pop(next(iter(self.voices)))
                self.voices[key]=ed.tts.voice(str(ref),reference)
            voice=self.voices[key]
            audio,settings=synthesize_omnivoice(ed.tts,text,voice,controls)
            output=self.projects.folder(identifier)/'synthesis.wav'
            write_wav_atomic(output,audio,24000)
            return {'name':'synthesis.wav','sample_rate':24000,'duration':len(audio)/24000,'shared_tts':True,'settings':settings}
        raise ValueError('Unsupported voice-editing action')


def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--root',type=Path,required=True);parser.add_argument('--models',type=Path,required=True)
    parser.add_argument('--manifest',type=Path,required=True);parser.add_argument('--tts',required=True)
    parser.add_argument('--steps',type=int,default=16);parser.add_argument('--threads',type=int,default=4)
    parser.add_argument('--device')
    parser.add_argument('--probe',action='store_true')
    parser.add_argument('--probe-reference',type=Path)
    a=parser.parse_args();a.root.mkdir(parents=True,exist_ok=True)
    import socket
    def offline(*args,**kwargs):raise OSError('Voice inference is offline')
    socket.socket.connect=offline;socket.create_connection=offline
    prepare_stdin()
    if a.probe and os.environ.get('ETHIOPIC_MANAGED_WORKER')=='1':
        def watch_parent():
            try:
                while read_stdin(1):pass
            finally:os._exit(1)
        threading.Thread(target=watch_parent,daemon=True).start()
    provider=Provider(a.root,a.models,json.loads(a.manifest.read_text(encoding='utf-8')),a.tts,a.steps,a.threads,a.device)
    if a.probe:
        with contextlib.redirect_stdout(sys.stderr):
            if a.probe_reference is None:raise ValueError('A shipped synthetic reference is required for the offline voice check')
            ed=provider.load()
            import numpy as np
            source=PCM.read(a.probe_reference);source=source.slice(0,min(source.frames,source.rate*6))
            reference=a.root/'probe-reference.wav';source.write(reference)
            recognized=ed.transcribe(source.mono(),sr=source.rate)
            if not recognized.get('text') or not recognized.get('words'):raise ValueError('Reference recognition probe failed')
            aligned=ed.align(source.mono(),recognized['text'],sr=source.rate)
            if len(aligned)!=len(recognized['text'].split()):raise ValueError('Reference alignment probe failed')
            project=provider.projects.create(reference,'Offline runtime check',recognized['text'],aligned)
            request={'projectId':project['id'],'revision':0,'seed':0}
            synthesis=provider.dispatch({**request,'action':'synthesize','text':'ሰላም ጤና ይስጥልኝ።','referenceText':recognized['text']})
            generated=PCM.read(provider.projects.folder(project['id'])/'synthesis.wav').mono()
            if not 6000<=len(generated)<=24000*60 or not np.isfinite(generated).all():raise ValueError('Speech generation probe failed')
            rms=float(np.sqrt(np.mean(generated.astype(np.float64)**2)))
            if rms<1e-5:raise ValueError('Speech generation probe produced silence')
            changed=recognized['text'].split();changed[0]='ጤና' if provider.editor._studio_norm(changed[0])!='ጤና' else 'ሰላም'
            preview=provider.dispatch({**request,'action':'preview','text':' '.join(changed)})
            if not preview.get('edits') or not preview.get('words'):raise ValueError('Speech editing probe failed')
            import torch
            memory={}
            if torch.cuda.is_available():
                torch.cuda.synchronize()
                memory={'cuda_peak_allocated_bytes':torch.cuda.max_memory_allocated(),
                        'cuda_peak_reserved_bytes':torch.cuda.max_memory_reserved()}
        print(json.dumps({'protocol':1,'probe':True,'shared_tts':ed.tts is provider.editor.tts,'synthesis_checked':True,
                          'alignment_checked':True,'edit_checked':True,'sample_rate':24000,'generated_frames':len(generated),
                          'duration':len(generated)/24000,'rms':rms,**memory}),flush=True)
        return
    protocol_out=sys.stdout
    requests=queue.Queue(maxsize=8)
    def read_parent():
        source=ProtocolInput()
        while True:
            line=source.readline(2*1024*1024+1)
            if not line:break
            if len(line)>2*1024*1024 or not line.endswith(b'\n'):os._exit(1)
            try:requests.put_nowait(json.loads(line))
            except Exception:os._exit(1)
        # Parent crash or shutdown must release resident CUDA/CPU memory even during inference.
        os._exit(0)
    threading.Thread(target=read_parent,daemon=True).start()
    print(json.dumps({'protocol':1,'ready':True}),flush=True)
    while True:
        req=requests.get();token=req.get('request')
        def progress(stage):
            print(json.dumps({'protocol':1,'request':token,'stage':stage}),file=protocol_out,flush=True)
        provider.progress=progress
        try:
            with contextlib.redirect_stdout(sys.stderr):result=provider.dispatch(req)
            print(json.dumps({'protocol':1,'request':token,'result':result},ensure_ascii=False,allow_nan=False),flush=True)
        except Exception as exc:
            kind=getattr(exc,'kind','failed')
            message=ERRORS.get(kind,'Voice editing could not finish. Your saved recording is unchanged.')
            # Never emit exception strings (model errors can contain full user transcripts).
            print(json.dumps({'protocol':1,'request':token,'error':{'kind':kind,'message':message}}),flush=True)


if __name__=='__main__':main()
