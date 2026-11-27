#!/usr/bin/env python3
"""Compare CPU adaptation to pinned official Mamba code, without CUDA imports.

This validation tool never supplies runtime app code. Official Python functions
and the entire Mamba class are executed unmodified from AST, with the official
selective_scan_ref replacing ONLY the compiled selective_scan_fn dispatch.
Fetch the three pinned sources separately; no implicit network requests occur.

Example (with the managed runtime site-packages on PYTHONPATH):
 python scripts/validate-cleanup-reference.py --mamba-source /tmp/reuse-mamba-v2.2.4.py \
   --scan-source /tmp/reuse-scan-v2.2.4.py --checkpoint .local-cleanup/source/model.safetensors
"""
from __future__ import annotations
import argparse
import ast
import hashlib
import importlib.util
import array
import wave
import types
import json
import math
from pathlib import Path
import sys
import time
from typing import Optional

import torch
from torch import nn, Tensor
from torch.nn import functional as F
from einops import rearrange, repeat
from safetensors import safe_open

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'workers' / 'cleanup'))
from mamba_reference import Mamba, selective_scan_reference


def official_def(path, name, namespace):
    text = path.read_text()
    tree = ast.parse(text, filename=str(path))
    node = next(item for item in tree.body if isinstance(item, (ast.ClassDef, ast.FunctionDef)) and item.name == name)
    exec(compile(ast.Module(body=[node], type_ignores=[]), str(path), 'exec'), namespace)
    return namespace[name]


def compare(actual, expected, rtol=1e-4, atol=2e-5):
    difference = (actual - expected).abs()
    scale = expected.abs().max().item()
    result = {'maxAbsoluteError':difference.max().item(), 'rootMeanSquareError':difference.square().mean().sqrt().item(),
              'normalizedMaxError':difference.max().item() / scale if scale > 0 else None}
    result.update({'relativeTolerance':rtol,'absoluteTolerance':atol,'passed':bool(torch.allclose(actual,expected,rtol=rtol,atol=atol))})
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--mamba-source', type=Path, required=True)
    parser.add_argument('--scan-source', type=Path, required=True)
    parser.add_argument('--checkpoint', type=Path, required=True)
    parser.add_argument('--threads', type=int, default=2)
    parser.add_argument('--output', type=Path)
    args = parser.parse_args()
    torch.set_num_threads(args.threads)
    torch.manual_seed(2026)
    namespace = {'torch':torch,'nn':nn,'F':F,'Tensor':Tensor,'Optional':Optional,'math':math,'rearrange':rearrange,'repeat':repeat,
                 'causal_conv1d_fn':None,'causal_conv1d_update':None,'selective_state_update':None}
    scan = official_def(args.scan_source, 'selective_scan_ref', namespace)
    namespace['selective_scan_fn'] = scan
    official = official_def(args.mamba_source, 'Mamba', namespace)
    report = {'oracle':'state-spaces/mamba v2.2.4 unmodified class slow path + official selective_scan_ref',
              'upstreamVersion':'v2.2.4','upstreamLicense':'Apache-2.0','upstreamSourceUrls':['https://raw.githubusercontent.com/state-spaces/mamba/v2.2.4/mamba_ssm/modules/mamba_simple.py','https://raw.githubusercontent.com/state-spaces/mamba/v2.2.4/mamba_ssm/ops/selective_scan_interface.py'],
              'device':'cpu','torchVersion':torch.__version__,'threads':args.threads,
              'provenance':{str(path):hashlib.sha256(path.read_bytes()).hexdigest() for path in (args.mamba_source,args.scan_source,args.checkpoint)}, 'comparisons':[]}
    with torch.inference_mode():
        u, dt, z = [torch.randn(2,8,75) for _ in range(3)]
        A = -torch.rand(8,4); B,C=[torch.randn(2,4,75) for _ in range(2)]; D,bias=torch.randn(8),torch.randn(8)
        result=compare(selective_scan_reference(u,dt,A,B,C,D,z,bias,True), scan(u,dt,A,B,C,D,z,bias,True))
        report['comparisons'].append({'stage':'selective-scan-toy',**result})
        with safe_open(str(args.checkpoint),framework='pt') as checkpoint:
            for name in ['TSMamba.0.time_mamba.forward_blocks','TSMamba.0.time_mamba.backward_blocks',
                         'TSMamba.0.freq_mamba.forward_blocks','TSMamba.29.freq_mamba.backward_blocks']:
                prefix=name+'.'
                weights={key[len(prefix):]:checkpoint.get_tensor(key) for key in checkpoint.keys() if key.startswith(prefix)}
                reference=official(64,d_state=16,d_conv=4,expand=4,use_fast_path=False).eval()
                adapted=Mamba(64,d_state=16,d_conv=4,expand=4).eval()
                reference.load_state_dict(weights,strict=True); adapted.load_state_dict(weights,strict=True)
                # Shapes mirror both reshape directions used by the full model;
                # normalized inputs match activation scale without fabricating audio.
                for shape in [(2,17,64),(9,81,64),(5,139,64)]:
                    x=F.layer_norm(torch.randn(*shape),(64,))
                    started=time.perf_counter(); expected=reference(x); oracle_seconds=time.perf_counter()-started
                    started=time.perf_counter(); actual=adapted(x); adapted_seconds=time.perf_counter()-started
                    report['comparisons'].append({'stage':name,'shape':shape,'strictWeights':True,
                        'officialSeconds':oracle_seconds,'adaptedSeconds':adapted_seconds,**compare(actual,expected)})
    # Validate first model-stage inputs from the released real audio sample,
    # including the encoder and bidirectional temporal block's actual output.
    source_dir = args.checkpoint.parent
    codec_path = source_dir / 'models' / 'codec_module_time_d4.py'
    sample_path = source_dir / 'noisy_audio' / 'mic_test2.wav'
    if codec_path.exists() and sample_path.exists():
        cfg = json.loads((source_dir / 'config.json').read_text())
        spec = importlib.util.spec_from_file_location('validation_codec', codec_path)
        codec = importlib.util.module_from_spec(spec); spec.loader.exec_module(codec)
        encoder = codec.DenseEncoder(cfg).eval()
        block_namespace = {'torch':torch,'nn':nn,'Mamba':official}
        block_source = source_dir / 'models' / 'mamba_block2_SEMamba.py'
        official_def(block_source, 'MambaBlock', block_namespace)
        block_class = official_def(block_source, 'TFMambaBlock', block_namespace)
        block = block_class(cfg).eval()
        with safe_open(str(args.checkpoint),framework='pt') as checkpoint:
            for module, prefix in [(encoder,'dense_encoder.'),(block,'TSMamba.0.')]:
                module.load_state_dict({key[len(prefix):]:checkpoint.get_tensor(key) for key in checkpoint.keys() if key.startswith(prefix)},strict=True)
        with wave.open(str(sample_path),'rb') as audio:
            if audio.getsampwidth()!=2 or audio.getnchannels()!=1: raise ValueError('Expected official mono PCM16 fixture')
            sr=audio.getframerate(); pcm=array.array('h',audio.readframes(int(sr * .15)))
        if sys.byteorder!='little': pcm.byteswap()
        signal=torch.tensor(pcm,dtype=torch.float32)[None,:]/32768
        stft_cfg=cfg['stft_cfg']
        def scaled(value):
            n=value*sr//stft_cfg['sampling_rate']; return n if n%2==0 else n+1
        nfft,hop,win=[scaled(stft_cfg[key]) for key in ('n_fft','hop_size','win_size')]
        with torch.inference_mode():
            spectrum=torch.stft(signal,nfft,hop_length=hop,win_length=win,window=torch.hann_window(win),center=True,pad_mode='reflect',return_complex=True)
            mag,phase=torch.log1p(spectrum.abs()),spectrum.angle()
            features=torch.stack((mag.transpose(1,2),phase.transpose(1,2)),dim=1)
            features=F.pad(features,(0,2,0,2)); encoded=encoder(features)
            b,c,t,f=encoded.shape
            time_input=encoded.permute(0,3,2,1).contiguous().view(b*f,t,c)
            time_output=block.time_mamba(time_input)+time_input
            freq_input=time_output.view(b,f,t,c).permute(0,2,1,3).contiguous().view(b*t,f,c)
            for axis,inputs in [('time',time_input),('freq',freq_input)]:
                for direction in ('forward_blocks','backward_blocks'):
                    oracle=getattr(getattr(block,axis+'_mamba'),direction)
                    adapted=Mamba(64,d_state=16,d_conv=4,expand=4).eval()
                    adapted.load_state_dict(oracle.state_dict(),strict=True)
                    x=inputs if direction=='forward_blocks' else inputs.flip(1)
                    started=time.perf_counter(); expected=oracle(x); oracle_seconds=time.perf_counter()-started
                    started=time.perf_counter(); actual=adapted(x); adapted_seconds=time.perf_counter()-started
                    report['comparisons'].append({'stage':f'actual-audio-first-{axis}-{direction}','sampleRate':sr,
                        'audioSeconds':len(pcm)/sr,'shape':list(x.shape),'officialSeconds':oracle_seconds,'adaptedSeconds':adapted_seconds,**compare(actual,expected)})
    # End-to-end SEMamba numerical oracle: identical released architecture and
    # checkpoint, with only the standard Mamba primitive implementation changed.
    # Use a real released recording slice, resampled for this validation input
    # with deterministic linear interpolation (not the app resampling policy).
    if codec_path.exists() and sample_path.exists():
        with wave.open(str(sample_path),'rb') as audio:
            rate=audio.getframerate(); pcm=array.array('h',audio.readframes(int(rate*.25)))
        if sys.byteorder!='little': pcm.byteswap()
        audio16=torch.tensor(pcm,dtype=torch.float32)[None,None,:]/32768
        audio16=F.interpolate(audio16,size=4000,mode='linear',align_corners=False).squeeze(1)
        stub=types.ModuleType('mamba_ssm'); stub.Mamba=official; sys.modules['mamba_ssm']=stub
        sys.path.insert(0,str(source_dir.resolve()))
        from models.generator_SEMamba_time_d4 import SEMamba
        import models.mamba_block2_SEMamba as block_module
        from models.stfts import mag_phase_stft, mag_phase_istft
        from safetensors.torch import load_file
        weights=load_file(str(args.checkpoint))
        reference_model=SEMamba(cfg).eval(); reference_model.load_state_dict(weights,strict=True)
        block_module.Mamba=Mamba
        adapted_model=SEMamba(cfg).eval(); adapted_model.load_state_dict(weights,strict=True)
        with torch.inference_mode():
            mag,phase,_=mag_phase_stft(audio16,640,80,640,'relu_log1p',center=True,addeps=False)
            started=time.perf_counter(); expected=reference_model(mag,phase); oracle_seconds=time.perf_counter()-started
            started=time.perf_counter(); actual=adapted_model(mag,phase); adapted_seconds=time.perf_counter()-started
            full={'sampleRate':16000,'audioSeconds':.25,'strictWeights':True,'officialSeconds':oracle_seconds,'adaptedSeconds':adapted_seconds}
            full['magnitude']=compare(actual[0],expected[0])
            # Phase is circular; equivalent phases differing by 2pi compare equal.
            phase_error=torch.atan2(torch.sin(actual[1]-expected[1]),torch.cos(actual[1]-expected[1]))
            full['phaseCircular']=compare(phase_error,torch.zeros_like(phase_error),rtol=0,atol=1e-3)
            full['phaseCircular']['thresholdRationale']='Absolute phase error under 0.001 radian (0.0573 degrees); waveform separately checked at rtol 0.0001 and atol 0.00002.'
            full['complex']=compare(actual[2],expected[2])
            # Apply the same published sweep suppression and inverse transform
            # independently, yielding final waveform comparison as well.
            reconstructed=[]
            for output in (expected,actual):
                amp,pha,_=output
                amp=amp.clone(); zero_fraction=(torch.expm1(F.relu(amp))==0).float().mean(dim=1)
                amp[:,:,(zero_fraction>.5)[0]]=0
                waveform=mag_phase_istft(amp,pha,640,80,640,'relu_log1p')
                waveform=F.pad(waveform,(0,max(0,4000-waveform.shape[-1])))[:,:4000]
                reconstructed.append(waveform)
            full['waveform']=compare(reconstructed[1],reconstructed[0])
            report['fullModel']=full
        for path in [source_dir/'config.json', source_dir/'models'/'generator_SEMamba_time_d4.py', source_dir/'models'/'mamba_block2_SEMamba.py',codec_path,source_dir/'models'/'stfts.py',Path(__file__).resolve().parents[1]/'workers'/'cleanup'/'mamba_reference.py']:
            report['provenance'][str(path)]=hashlib.sha256(path.read_bytes()).hexdigest()
    report['passed']=all(item['passed'] for item in report['comparisons']) and all(report.get('fullModel',{}).get(key,{'passed':False})['passed'] for key in ['magnitude','phaseCircular','complex','waveform'])
    text=json.dumps(report,indent=2)
    print(text)
    if args.output: args.output.write_text(text+'\n')
    if not report['passed']: raise SystemExit(1)

if __name__=='__main__': main()
