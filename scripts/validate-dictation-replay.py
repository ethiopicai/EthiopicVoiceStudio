#!/usr/bin/env python3
"""Replay a short recorded fixture in real time through the installed dictation worker.

This is not a microphone or accuracy test. The report contains timings/counts, not
recognized text. The temporary recognition JSON is removed after validation.
"""
import argparse
import hashlib
import json
import math
import os
from pathlib import Path
import queue
import subprocess
import tempfile
import threading
import time
import wave


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--installation', type=Path, required=True)
    parser.add_argument('--audio', type=Path, required=True)
    parser.add_argument('--report', type=Path, required=True)
    args = parser.parse_args()
    import numpy as np
    with wave.open(str(args.audio)) as file:
        if (file.getnchannels(), file.getframerate(), file.getsampwidth()) != (1, 16000, 2):
            raise ValueError('Use a 16 kHz mono PCM16 replay fixture')
        audio = np.frombuffer(file.readframes(file.getnframes()), '<i2').astype(np.float32) / 32768
    if not 0 < len(audio) <= 16000 * 60:
        raise ValueError('Use a fixture up to one minute')
    root = args.installation.resolve()
    config = json.loads((root / 'installation.json').read_text())
    worker = Path(__file__).resolve().parent.parent / 'workers/asr/dictation_worker.py'
    with tempfile.TemporaryDirectory(prefix='dictation-replay-') as folder:
        result = Path(folder) / 'result.json'
        command = [str(root / config['python']), '-I', '-B', '-u', '-c',
                   "import sys,runpy;sys.path.insert(0,sys.argv.pop(1));runpy.run_path(sys.argv.pop(1),run_name='__main__')",
                   str(root / config['site_packages']), str(worker), '--bundle', str(root / 'bundle'), '--output', str(result)]
        process = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                                   env={**os.environ, 'ETHIOPIC_MANAGED_WORKER':'1'})
        messages = queue.Queue()
        def read():
            for line in process.stdout:
                messages.put(json.loads(line))
            messages.put(None)
        threading.Thread(target=read, daemon=True).start()
        started = time.monotonic()
        ready = None
        finals, partial_events, delays = [], 0, []
        sender_error = []
        sender = None
        audio_start = None
        def send():
            try:
                for at in range(0, len(audio), 1600):
                    end = min(at + 1600, len(audio))
                    # Deliver a piece once its samples would have been recorded.
                    wait = audio_start + end / 16000 - time.monotonic()
                    if wait > 0:
                        time.sleep(wait)
                    process.stdin.write((json.dumps({'type':'audio','samples':audio[at:end].tolist()})+'\n').encode())
                    process.stdin.flush()
                process.stdin.write(b'{"type":"finish"}\n')
                process.stdin.flush()
            except Exception as exc:
                sender_error.append(type(exc).__name__)
        try:
            while True:
                message = messages.get(timeout=120)
                if message is None:
                    break
                if 'error' in message:
                    raise RuntimeError(message['error'])
                if message.get('event') == 'ready':
                    ready = message
                    audio_start = time.monotonic()
                    sender = threading.Thread(target=send)
                    sender.start()
                elif message.get('event') == 'transcript':
                    # Exercise the same monotonic boundaries checked by Studio,
                    # including the boundary between final words and the grey tail.
                    boundary = finals[-1]['end'] if finals else 0.0
                    for word in message['final'] + message['partial']:
                        if not (math.isfinite(word['start']) and math.isfinite(word['end'])
                                and boundary <= word['start'] <= word['end'] <= len(audio)/16000 + .04):
                            raise RuntimeError('Replay returned overlapping or invalid word timings')
                        boundary = word['end']
                    finals.extend(message['final'])
                    partial_events += bool(message['partial'])
                    delays.extend(time.monotonic() - audio_start - word['end'] for word in message['final'])
                elif message.get('event') == 'finished':
                    break
            if sender:
                sender.join(timeout=5)
            if sender_error or process.wait(timeout=10) != 0 or ready is None:
                raise RuntimeError('Replay worker did not complete')
            saved = json.loads(result.read_text())
            if saved['words'] != finals or saved['model_version'] != ready['model_version']:
                raise RuntimeError('Final append-only events do not match saved transcript')
            report = {'test':'recorded_audio_replayed_in_real_time_not_live_microphone',
                      'model_version':saved['model_version'], 'device':ready['device'], 'dtype':ready['dtype'],
                      'fixture_sha256':hashlib.sha256(args.audio.read_bytes()).hexdigest(),
                      'audio_seconds':len(audio)/16000, 'startup_seconds':audio_start-started,
                      'decode_7_5_seconds':ready['decode_seconds'], 'wall_seconds':time.monotonic()-started,
                      'final_word_count':len(finals), 'events_with_partial_words':partial_events,
                      'word_final_delay_seconds':{'minimum':min(delays) if delays else None,
                                                  'maximum':max(delays) if delays else None,
                                                  'mean':sum(delays)/len(delays) if delays else None},
                      'append_only_matches_saved':True,'accuracy_measured':False,'passed':True}
            args.report.parent.mkdir(parents=True,exist_ok=True)
            args.report.write_text(json.dumps(report,indent=2)+'\n')
            print(json.dumps(report,indent=2))
        finally:
            if process.poll() is None:
                process.kill()
            process.wait()
            process.stdin.close()
            process.stdout.close()


if __name__ == '__main__':
    main()
