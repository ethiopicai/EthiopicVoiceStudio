#!/usr/bin/env python3
"""Maintainer-only offline contract/latency check. Never logs recognized or corrected text."""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import resource
import socket
import sys
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source', type=Path, default=Path('.local-studio'))
    parser.add_argument('--site-packages', type=Path, default=Path('.local-studio/onnx-site-packages'))
    parser.add_argument('--variant', required=True, choices=['asr-wav2vec_ctc-1b-amh-int8_onnx', 'asr-wav2vec_ctc-300m-amh-int8_onnx', 'textcorrect-char_bart-7m-amh-int8_onnx'])
    parser.add_argument('--audio', type=Path, default=Path('.local-asr/validation.wav'))
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    os.environ['ORT_DISABLE_TELEMETRY'] = '1'
    sys.path.insert(0, str(args.site_packages.resolve()))
    if importlib.util.find_spec('torch') is not None:
        raise RuntimeError('Run in a Torch-free isolated Python environment')
    def deny(*unused_args, **unused_kwargs):
        raise OSError('Network disabled during model validation')
    socket.socket.connect = deny
    socket.create_connection = deny
    import onnxruntime as ort
    ort.disable_telemetry_events()
    manifest = json.loads((args.source / 'manifest.json').read_text())
    variant = next(v for v in manifest['variants'] if v['id'] == args.variant)
    folder = args.source / variant['folder']
    for entry in variant['files']:
        target = folder / entry['path']
        with target.open('rb') as stream:
            actual = hashlib.file_digest(stream, 'sha256').hexdigest()
        if actual != entry['sha256'] or target.stat().st_size != entry['bytes']:
            raise ValueError('Source variant failed its manifest check')
    filename = 'asr_onnx_infer.py' if variant['component'] == 'asr' else 'corrector_onnx.py'
    spec = importlib.util.spec_from_file_location('reference_provider', folder / filename)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    import numpy as np
    before = time.monotonic()
    if variant['component'] == 'asr':
        model = module.OnnxCTCTranscriber(str(folder), threads=4)
        load_seconds = time.monotonic() - before
        audio = module.load_audio(str(args.audio))
        audio = np.tile(audio, max(1, int(np.ceil(160000 / len(audio)))) )[:160000]
        before = time.monotonic()
        result = model.transcribe(audio)
        seconds = time.monotonic() - before
        if result['model_version'] != variant['model_version']:
            raise ValueError('Reference model version is absent or inconsistent; variant.json must be provided')
        if result['duration'] != 10.0:
            raise ValueError('Unexpected prepared audio duration')
        previous = 0.0
        for word in result['words']:
            if not word['word'].strip() or not previous <= word['start'] <= word['end'] <= 10.04:
                raise ValueError('Invalid word timing')
            previous = word['end']
        before = time.monotonic()
        update = model.decode(audio[:64000], offset=1.0)
        update_seconds = time.monotonic() - before
        if any(not 1.0 <= w['start'] <= w['end'] <= 5.04 for w in update):
            raise ValueError('Decode offsets are inconsistent')
        info = {'model': variant['model'], 'model_version': result['model_version'],
                'prepared_audio_seconds': 10, 'inference_seconds': round(seconds, 3),
                'word_count': len(result['words']), 'decode_4s_seconds': round(update_seconds, 3)}
    else:
        model = module.OnnxCorrector(str(folder), threads=2)
        load_seconds = time.monotonic() - before
        texts = ['ሰላም ነው እንዴት ነህ', 'ይህ test 123 ነው']
        before = time.monotonic()
        result = model(texts, mode='punct')
        seconds = time.monotonic() - before
        if [module.strip_punct(t).split() for t in result] != [module.strip_punct(t).split() for t in texts]:
            raise ValueError('Punctuation mode changed source words')
        typed = model(texts, mode='full')
        if len(typed) != len(texts) or any(not isinstance(t, str) for t in typed):
            raise ValueError('Incorrect typed-text result shape')
        info = {'punctuation_preserves_words': True, 'tested_texts': len(texts), 'punctuation_batch_seconds': round(seconds, 3)}
    if 'torch' in sys.modules:
        raise RuntimeError('Torch unexpectedly imported')
    info.update(variant=args.variant, load_seconds=round(load_seconds, 3),
                numpy=np.__version__, onnxruntime=ort.__version__, network='disabled', torch_importable=False, ort_telemetry='disabled before initialization',
                max_rss_bytes=resource.getrusage(resource.RUSAGE_SELF).ru_maxrss,
                validation='local macOS provider smoke test; not a quality or low-end qualification')
    args.output.write_text(json.dumps(info, indent=2) + '\n')
    print(json.dumps(info))


if __name__ == '__main__':
    main()
