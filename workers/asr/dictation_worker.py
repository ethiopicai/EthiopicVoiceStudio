"""One offline dictation session. Private IPC carries words, never diagnostic logs.

stdin: bounded newline JSON audio chunks, then finish. A reader thread queues audio;
only the inference thread touches StreamingTranscriber. All queued chunks are joined
before each feed. EOF without finish means the parent disappeared, not successful stop.
"""
import argparse
from collections import deque
import contextlib
import json
import math
import os
from pathlib import Path
import sys
import threading
import time
from provider import prepare_stdin, read_stdin

PROTOCOL_OUT = sys.stdout
RATE = 16000
MAX_CHUNK = 32000
MAX_QUEUED = RATE * 10
MAX_SAMPLES = RATE * 1800
MAX_MESSAGE = 2 * 1024 * 1024


def event(**fields):
    print(json.dumps({'protocol': 1, **fields}, ensure_ascii=False, allow_nan=False), file=PROTOCOL_OUT, flush=True)


class AudioQueue:
    def __init__(self, sample_rate=RATE):
        if sample_rate not in (16000, 48000):
            raise ValueError('invalid_sample_rate')
        self.rate = sample_rate
        self.condition = threading.Condition()
        self.parts = deque()
        self.queued = self.total = 0
        self.finished = False

    def push(self, values):
        import numpy as np
        if not isinstance(values, list) or not 0 < len(values) <= self.rate * 2:
            raise ValueError('invalid_audio')
        if any(isinstance(value, bool) or not isinstance(value, (int, float)) for value in values):
            raise ValueError('invalid_audio')
        audio = np.asarray(values, dtype=np.float32)
        if not np.isfinite(audio).all() or (np.abs(audio) > 1).any():
            raise ValueError('invalid_audio')
        with self.condition:
            if self.finished:
                raise ValueError('invalid_sequence')
            if self.total + len(audio) > self.rate * 1800:
                raise ValueError('recording_limit')
            if self.queued + len(audio) > self.rate * 10:
                raise ValueError('backlog_exceeded')
            self.parts.append(audio)
            self.queued += len(audio)
            self.total += len(audio)
            self.condition.notify()

    def finish(self):
        with self.condition:
            if self.finished:
                raise ValueError('invalid_sequence')
            self.finished = True
            self.condition.notify()

    def take(self, minimum=0):
        import numpy as np
        with self.condition:
            while (not self.parts or self.queued < minimum) and not self.finished:
                self.condition.wait()
            # Drain the entire backlog, not one old chunk per expensive decode.
            parts = list(self.parts)
            self.parts.clear()
            self.queued = 0
            done = self.finished
        return (np.concatenate(parts) if parts else None), done


class RawInput:
    """Bounded stdin framing without buffered Python locks on parent loss."""
    def __init__(self):
        prepare_stdin()
        self.buffer = bytearray()

    def readline(self, limit):
        while True:
            newline = self.buffer.find(b'\n')
            if newline >= 0:
                count = min(newline + 1, limit)
                line = bytes(self.buffer[:count])
                del self.buffer[:count]
                return line
            if len(self.buffer) >= limit:
                line = bytes(self.buffer[:limit])
                del self.buffer[:limit]
                return line
            piece = read_stdin(min(65536, limit - len(self.buffer)))
            if not piece:
                line = bytes(self.buffer)
                self.buffer.clear()
                return line
            self.buffer.extend(piece)


def read_messages(queue, source, fatal, watch_parent=None):
    try:
        while True:
            line = source.readline(MAX_MESSAGE + 1)
            if not line:
                raise ValueError('parent_closed')
            if len(line) > MAX_MESSAGE or not line.endswith(b'\n'):
                raise ValueError('invalid_message')
            message = json.loads(line)
            if not isinstance(message, dict):
                raise ValueError('invalid_message')
            if message.get('type') == 'audio' and set(message) == {'type', 'samples'}:
                queue.push(message['samples'])
            elif message == {'type': 'finish'}:
                queue.finish()
                if watch_parent is not None:
                    watch_parent()
                return
            else:
                raise ValueError('invalid_message')
    except Exception as exc:
        code = str(exc) if str(exc) in {'parent_closed', 'backlog_exceeded', 'recording_limit'} else 'worker_failed'
        fatal(code)


def checked_words(words, duration):
    if not isinstance(words, list):
        raise ValueError('invalid_alignment')
    result = []
    last = 0.0
    for word in words:
        text, start, end = word['word'], word['start'], word['end']
        if not isinstance(text, str) or not text.strip() or len(text) > 4096:
            raise ValueError('invalid_alignment')
        if not (math.isfinite(start) and math.isfinite(end) and last <= start <= end <= duration + .04):
            raise ValueError('invalid_alignment')
        result.append({'word': text, 'start': start, 'end': end})
        last = end
    return result


def after_boundary(words, boundary):
    """Reconcile a rolling decode with immutable, already published words.

    Context is decoded again and its word edges can move across the last final
    end. Ignore spans entirely in that finalized context; clip only the start
    of a new, straddling word. Never change a previously published final span.
    """
    return [dict(word, start=max(boundary, word['start']))
            for word in words if word['end'] > boundary]


def consume(stream, queue, emit, preprocessor=None):
    final = []
    processed = 0
    def publish(value):
        boundary = final[-1]['end'] if final else 0.0
        settled = after_boundary(checked_words(value['final'], processed / RATE), boundary)
        boundary = settled[-1]['end'] if settled else boundary
        partial = after_boundary(checked_words(value['partial'], processed / RATE), boundary)
        # Both IPC and the saved transcript use these same append-only spans.
        final.extend(settled)
        emit(event='transcript', final=settled, partial=partial, audio_seconds=processed / RATE)
    def feed(audio):
        nonlocal processed
        if len(audio):
            processed += len(audio)
            value = stream.feed(audio)
            if value is not None:
                publish(value)
    while True:
        # Coalesce half a second for the denoiser's efficient inference blocks.
        # This bounds added batching latency, rather than running a GPU call for
        # every tiny microphone chunk or silently dropping a slow backlog.
        audio, done = queue.take(queue.rate // 2 if preprocessor is not None else 0)
        if audio is not None:
            feed(preprocessor.push(audio) if preprocessor is not None else audio)
        if done:
            if preprocessor is not None:
                feed(preprocessor.finish())
            publish(stream.finish())
            return final, processed


def run(bundle, output, sample_rate=RATE, noise_config=None):
    os.environ.update(ORT_DISABLE_TELEMETRY='1', HF_HUB_OFFLINE='1', TRANSFORMERS_OFFLINE='1', HF_HUB_DISABLE_TELEMETRY='1',
                      TOKENIZERS_PARALLELISM='false', PYTHONDONTWRITEBYTECODE='1')
    import socket
    def deny(*args, **kwargs):
        raise OSError('Network access is disabled in the dictation worker')
    socket.socket.connect = deny
    socket.create_connection = deny
    from provider import prepare_private_runtime
    prepare_private_runtime()
    prepare_stdin()
    queue = AudioQueue(sample_rate)
    def fatal(code):
        event(error=code)
        # Stop ongoing inference too when stdin closes or the bounded queue fails.
        os._exit(1)
    def watch_parent():
        try:
            while read_stdin(1):
                pass
        finally:
            fatal('parent_closed')
    reader = threading.Thread(target=read_messages, args=(queue, RawInput(), fatal, watch_parent), daemon=True)
    reader.start()
    event(stage='Loading Amharic voice tools')
    with contextlib.redirect_stdout(sys.stderr):
        import numpy as np
        from provider import load_provider
        asr, stream_factory = load_provider(bundle, 'dictation')
        asr.decode(np.zeros(RATE, np.float32))
        started = time.monotonic()
        asr.decode(np.zeros(int(7.5 * RATE), np.float32))
        decode_seconds = time.monotonic() - started
        stream = stream_factory(asr)
        noise = None
        if noise_config is not None:
            if sample_rate != 48000:
                raise ValueError('noise_requires_48000')
            from noise_reduction import NoiseReducer
            noise = NoiseReducer(json.loads(noise_config.read_text()))
        elif sample_rate != RATE:
            raise ValueError('invalid_sample_rate')
        preprocessor = noise.begin() if noise is not None else None
    event(event='ready', model_version=asr.version, decode_seconds=decode_seconds,
          device=asr._studio_device, dtype=asr._studio_dtype)
    with contextlib.redirect_stdout(sys.stderr):
        words, count = consume(stream, queue, event, preprocessor)
    if count < 400:
        raise ValueError('recording_too_short')
    result = {'text': ' '.join(word['word'] for word in words), 'words': words,
              'duration': count / RATE, 'model_version': asr.version}
    if noise is not None:
        result['processing'] = {'noise_reduction': noise.metadata()}
    # This one-file worker is also staged as worker.py; do not import itself as a helper.
    # Native transcript validation checks the optional family before committing it.
    if asr._studio_model_name is not None:
        result['model'] = asr._studio_model_name
    pending = output.with_suffix('.pending')
    with pending.open('w', encoding='utf-8') as file:
        json.dump(result, file, ensure_ascii=False, allow_nan=False)
        file.flush()
        os.fsync(file.fileno())
    os.replace(pending, output)
    event(event='finished', model_version=asr.version)


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--bundle', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--sample-rate', type=int, default=RATE)
    parser.add_argument('--noise-config', type=Path)
    args = parser.parse_args()
    try:
        run(args.bundle, args.output, args.sample_rate, args.noise_config)
    except Exception as exc:
        if os.environ.get('ETHIOPIC_ASR_DEVELOPER_DIAGNOSTICS') == '1':
            import traceback
            traceback.print_exc(file=sys.stderr)
        code = ('out_of_memory' if isinstance(exc, MemoryError) or 'out of memory' in str(exc).lower()
                else str(exc) if str(exc) in {'gpu_unavailable', 'gpu_precision_unsupported'} else 'worker_failed')
        event(error=code)
        sys.exit(1)
