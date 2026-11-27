#!/usr/bin/env python3
"""Build the licensed, noncommercial preview voice pack with local PocketTTS.

Run using the Studio managed Python and PYTHONPATH. Source metadata is pinned;
the large synthesis model and original sample audio are never bundled in the UI.
"""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import time
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
WORK = ROOT / '.local-tts/reference-design'
METADATA = ROOT / 'scripts/reference-voices'
OUT = ROOT / 'apps/desktop/ui/public/reference-voices/preview-v3'
TEXT = (METADATA / 'preview-text.txt').read_text().rstrip('\n')
REPO = 'ABBNDZ/ATTS1HG1'
REVISION = '04d285138f0c2f8a7939dd9a3e48d8be12dc1712'


def digest(path):
    return hashlib.file_digest(open(path, 'rb'), 'sha256').hexdigest()


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--model', type=Path, default=ROOT / '.local-tts/models/tts-flow_lm-90m-amh-fp32')
    ap.add_argument('--limit', type=int)
    args = ap.parse_args()
    import numpy as np
    import soundfile as sf
    import torch
    os.environ['HF_HUB_OFFLINE'] = '1'
    os.environ['TRANSFORMERS_OFFLINE'] = '1'
    OUT.mkdir(parents=True, exist_ok=True)
    sources = WORK / 'sources'
    sources.mkdir(exist_ok=True)
    metadata = json.loads((METADATA / 'source-model.json').read_text())
    source_text = {x['output']['url'].rsplit('/', 1)[1]: x['text'] for x in metadata['widgetData']}
    files = json.loads((METADATA / 'source-files.json').read_text())
    personas = {persona['sourcePersonaId']:persona for persona in json.loads((METADATA / 'personas.json').read_text())['personas']}
    legacy = {voice['id']:voice for voice in json.loads((METADATA / 'preview-v1-catalog.json').read_text())['voices']}
    prior_catalogs = [json.loads((METADATA / f'preview-v{version}-catalog.json').read_text()) for version in (1, 2)]
    prior_defaults = {catalog['packId']: {voice['id']: {**{key:voice[key] for key in ['name','image','referenceText','bytes','audioSha256']}, 'audioName': Path(voice['audio']).name} for voice in catalog['voices']} for catalog in prior_catalogs}
    selected = [x for x in files if x['path'].endswith('.wav')]
    if args.limit:
        selected = selected[:args.limit]
    spec = importlib.util.spec_from_file_location('preview_tts', args.model / 'tts_small_infer.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    tts = module.AmharicTTS(threads=4, int8=False, temp=0.7)
    base_steps = tts.model.sampler_decode_steps
    if not isinstance(base_steps, int) or base_steps < 1:
        raise ValueError('The model did not expose a valid baseline flow-step count.')
    tts.model.sampler_decode_steps = base_steps * 2
    weights_hash = digest(args.model / 'model.safetensors')
    print(f'Flow/diffusion steps: {base_steps} -> {tts.model.sampler_decode_steps}', flush=True)
    previous = {voice['id']:voice for voice in json.loads((OUT / 'catalog.json').read_text())['voices']} if (OUT / 'catalog.json').exists() else {}
    voices = []
    persona_counts = {}
    for index, entry in enumerate(selected):
        filename = entry['path'].rsplit('/', 1)[1]
        source = sources / filename
        url = f'https://huggingface.co/{REPO}/resolve/{REVISION}/{entry["path"]}'
        if not source.exists() or digest(source) != entry['lfs']['oid']:
            partial = source.with_suffix('.wav.part')
            for attempt in range(4):
                try:
                    with urllib.request.urlopen(url, timeout=90) as response, partial.open('wb') as destination:
                        shutil.copyfileobj(response, destination)
                    if digest(partial) != entry['lfs']['oid']:
                        raise ValueError(f'Source checksum mismatch: {filename}')
                    partial.replace(source)
                    break
                except Exception:
                    if attempt == 3:
                        raise
                    time.sleep(2 ** attempt)
        stem, language = filename[:-4].rsplit('_', 1)
        source_persona = stem.replace('_', '-').lower()
        persona = personas[source_persona]
        persona_id = persona['name'].lower()
        voice_id = f'preview-{source_persona}-{language}'
        persona_counts[source_persona] = persona_counts.get(source_persona,0) + 1
        has_alternates = sum(f['path'].rsplit('/',1)[1].startswith(stem+'_') for f in files) > 1
        variant_number = persona_counts[source_persona]
        output = OUT / f'{persona_id}_{variant_number}_am.wav'
        seed = 20261119 + index
        started = time.monotonic()
        cached = previous.get(voice_id,{})
        gain = cached.get('generation',{}).get('outputGain',1.0)
        cache_matches = cached.get('referenceText') == TEXT and cached.get('generation',{}).get('samplerDecodeSteps') == tts.model.sampler_decode_steps and cached.get('generation',{}).get('weightsSha256') == weights_hash and cached.get('source',{}).get('sha256') == entry['lfs']['oid']
        if not output.exists() or not cache_matches or digest(output) != cached.get('audioSha256'):
            tts._voices.clear()
            with torch.random.fork_rng(devices=[]):
                torch.manual_seed(0)
                tts._voice_state(str(source))
            torch.manual_seed(seed)
            waveform = tts.synth(TEXT, voice=str(source))
            if not np.isfinite(waveform).all() or np.max(np.abs(waveform)) < 0.001:
                raise ValueError(f'Invalid generated waveform: {filename}')
            gain = min(1.0, .98 / float(np.max(np.abs(waveform))))
            sf.write(output, waveform * gain, tts.sample_rate, subtype='PCM_16')
        waveform, sr = sf.read(output, dtype='float32')
        duration = len(waveform) / sr
        if waveform.ndim != 1 or sr != 24000 or duration < legacy[voice_id]['durationSeconds'] + 12 or duration > 120 or not np.isfinite(waveform).all():
            raise ValueError(f'Invalid generated audio: {output}')
        voices.append({
            'id': voice_id, 'personaId': persona_id,
            'name': f'{persona["name"]} · {persona["amharicName"]}' + (f' {variant_number}' if has_alternates else ''),
            'image': persona['image'],
            'avatarPresentation': persona['avatarPresentation'],
            'avatarSelectionBasis': 'fictional-ethiopian-persona', 'genderVerified': False,
            'language': 'am-ET', 'referenceText': TEXT,
            'audio': f'/reference-voices/preview-v3/{output.name}',
            'audioSha256': digest(output), 'bytes': output.stat().st_size,
            'sampleRate': sr, 'durationSeconds': round(duration, 6),
            'peak': float(np.max(np.abs(waveform))),
            'rms': float(np.sqrt(np.mean(waveform ** 2))),
            'longerThanV1Seconds': round(duration - legacy[voice_id]['durationSeconds'],6),
            'previousDefault': {key:legacy[voice_id][key] for key in ['name','image','referenceText','bytes','audioSha256']},
            'previousAudioName': Path(legacy[voice_id]['audio']).name,
            'previousDefaults': {pack_id: defaults[voice_id] for pack_id, defaults in prior_defaults.items()},
            'source': {'repository': REPO, 'revision': REVISION, 'filename': filename,
                       'language': language, 'sha256': entry['lfs']['oid'], 'url': url,
                       'transcript': source_text[filename]},
            'generation': {'model': 'ethiopic/ethiopic-studio-models/tts-flow_lm-90m-amh-fp32',
                           'weightsSha256': weights_hash,
                           'baseSamplerDecodeSteps': base_steps, 'samplerDecodeSteps': tts.model.sampler_decode_steps,
                           'stepMultiplier': 2,
                           'temperature': 0.7, 'seed': seed, 'promptSeed': 0, 'outputGain': gain,
                           'sourceConditioningMaxSeconds': 5,
                           'sourceConditioningSeconds': min(5,sf.info(source).duration), 'sampleRate': sr,
                           'runtime': f'torch {torch.__version__}; PocketTTS; Apple Silicon CPU FP32'},
            'license': {'id': 'CPML-1.0', 'noncommercial': True,
                        'url': 'https://coqui.ai/cpml',
                        'notice': '/reference-voices/preview-v3/LICENSES.txt'}
        })
        print(f'{index+1}/{len(selected)} {output.name}: {duration:.2f}s, peak {voices[-1]["peak"]:.3f}, generated in {time.monotonic()-started:.1f}s', flush=True)
    (OUT / 'catalog.json').write_text(json.dumps({'schemaVersion': 3, 'packId': 'atts1hg1-am-preview-v3', 'replacesPackIds': list(prior_defaults), 'noncommercial': True, 'text': TEXT, 'voices': voices}, ensure_ascii=False, indent=2)+'\n')
    (OUT / 'LICENSES.txt').write_text('Ethiopic Voice Studio — noncommercial preview reference voices\n\n'
        'Source: ABBNDZ/ATTS1HG1 samples, revision '+REVISION+'\n'
        'https://huggingface.co/ABBNDZ/ATTS1HG1\n'
        'Derived from Coqui XTTS-v2 outputs. The Coqui Public Model License limits these samples and derived preview outputs to noncommercial use.\n'
        'Generated with Ethiopic Amharic PocketTTS (tts-flow_lm-90m-amh-fp32), whose released fine-tune is noncommercial.\n'
        'Names and portraits are AI-generated fictional Ethiopian personas, not the source speakers or verified speaker identities.\n'
        'The original sample audio and the synthesis model weights are not included in this pack.\n\n'
        +(METADATA / 'LICENSE-CPML.txt').read_text())


if __name__ == '__main__':
    main()
