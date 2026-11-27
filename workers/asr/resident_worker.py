"""Private resident offline ASR worker, protocol v1.

One input reader owns stdin for the entire process. The inference thread alone owns
CTC/stream state. Request completion is finished -> cleared request state -> idle.
EOF is parent loss even after finish: there is no successful EOF shutdown protocol.
"""
import argparse
import contextlib
import json
import math
import os
from pathlib import Path
import sys
import threading
import time

from worker import load_audio, annotate_model
from dictation_worker import AudioQueue, RawInput, MAX_MESSAGE, RATE, consume, checked_words
from provider import load_provider, provider_key, release_cache, streaming_factory, prepare_private_runtime, prepare_stdin

PROTOCOL_OUT = sys.stdout


def event(**fields):
    print(json.dumps({'protocol': 1, **fields}, ensure_ascii=False, allow_nan=False),
          file=PROTOCOL_OUT, flush=True)


def protect_offline():
    os.environ.update(ORT_DISABLE_TELEMETRY='1', HF_HUB_OFFLINE='1', TRANSFORMERS_OFFLINE='1', HF_HUB_DISABLE_TELEMETRY='1',
                      TOKENIZERS_PARALLELISM='false', PYTHONDONTWRITEBYTECODE='1')
    import socket
    def deny(*args, **kwargs):
        raise OSError('Network access is disabled in the ASR worker')
    socket.socket.connect = deny
    socket.create_connection = deny
    prepare_private_runtime()
    prepare_stdin()


def benchmark(model):
    import numpy as np
    model.decode(np.zeros(RATE, np.float32))
    started = time.monotonic()
    model.decode(np.zeros(int(7.5 * RATE), np.float32))
    return time.monotonic() - started


def atomic_result(output, result):
    pending = output.with_suffix('.pending')
    with pending.open('w', encoding='utf-8') as file:
        json.dump(result, file, ensure_ascii=False, allow_nan=False)
        file.flush()
        os.fsync(file.fileno())
    os.replace(pending, output)


def error_code(exc):
    if isinstance(exc, MemoryError) or 'out of memory' in str(exc).lower():
        return 'out_of_memory'
    return str(exc) if str(exc) in {'parent_closed', 'backlog_exceeded', 'recording_limit',
                                  'gpu_unavailable', 'gpu_precision_unsupported'} else 'worker_failed'


class Requests:
    """Single request slot; audio is bounded by AudioQueue rather than stdin buffering."""
    def __init__(self, fatal):
        self.condition = threading.Condition()
        self.request = None
        self.audio = None
        self.claimed = False
        self.failure = None
        self.fatal = fatal

    def fail(self, exc):
        code = error_code(exc)
        with self.condition:
            if self.failure is not None:
                return
            self.failure = code
            self.condition.notify_all()
        self.fatal(code)

    def accept(self, message):
        if not isinstance(message, dict):
            raise ValueError('invalid_message')
        with self.condition:
            kind = message.get('type')
            if kind in {'transcribe', 'dictation'}:
                required = {'type', 'audio', 'output'} if kind == 'transcribe' else {'type', 'output'}
                optional = {'noise_reduction'} | ({'sample_rate'} if kind == 'dictation' else set())
                if self.request is not None or not required <= set(message) or set(message) - required - optional:
                    raise ValueError('invalid_sequence')
                rate = message.get('sample_rate', RATE)
                if isinstance(rate, bool) or rate not in (RATE, 48000):
                    raise ValueError('invalid_sample_rate')
                for name in required - {'type'}:
                    value = message[name]
                    if not isinstance(value, str) or not value or len(value) > 4096 or '\0' in value:
                        raise ValueError('invalid_path')
                self.request = dict(message)
                self.audio = AudioQueue(rate) if kind == 'dictation' else None
                self.claimed = False
                self.condition.notify_all()
            elif kind == 'audio' and set(message) == {'type', 'samples'} and self.audio is not None:
                self.audio.push(message['samples'])
            elif message == {'type': 'finish'} and self.audio is not None:
                self.audio.finish()
            else:
                raise ValueError('invalid_sequence')

    def take(self):
        with self.condition:
            while (self.request is None or self.claimed) and self.failure is None:
                self.condition.wait()
            if self.failure is not None:
                raise RuntimeError(self.failure)
            self.claimed = True
            return self.request, self.audio

    def clear(self):
        with self.condition:
            if self.failure is not None:
                raise RuntimeError(self.failure)
            self.request = None
            self.audio = None
            self.claimed = False


def read_requests(requests, source):
    try:
        while True:
            line = source.readline(MAX_MESSAGE + 1)
            if not line:
                raise ValueError('parent_closed')
            if len(line) > MAX_MESSAGE or not line.endswith(b'\n'):
                raise ValueError('invalid_message')
            message = json.loads(line)
            requests.accept(message)
            # Do not retain user samples/paths in the blocked reader's local scope.
            message = None
            line = None
    except Exception as exc:
        requests.fail(exc)


class Resident:
    """Only the provider and first-live readiness measurement survive a request."""
    def __init__(self, bundle, emit=event, model_factory=load_provider,
                 warmup=benchmark, audio_loader=load_audio):
        self.bundle = Path(bundle)
        self.emit = emit
        self.model_factory = model_factory
        self.warmup = warmup
        self.audio_loader = audio_loader
        self.model = None
        self.model_key = None
        self.stream_factory = None
        self.decode_seconds = None
        self.noise = None

    def noise_provider(self, request):
        config = request.get('noise_reduction')
        if config is None:
            return None
        if self.noise is None or self.noise.config != config:
            self.emit(stage='Preparing noise reduction')
            from noise_reduction import NoiseReducer
            self.noise = NoiseReducer(config)
        return self.noise

    def provider(self, task='transcription'):
        key = provider_key(self.bundle, task) if self.model_factory is load_provider else 'custom'
        if self.model is not None and key != self.model_key:
            device = getattr(self.model, '_studio_device', 'cpu')
            self.model = self.stream_factory = None
            self.decode_seconds = None
            release_cache(device)
        if self.model is None:
            self.emit(stage='Loading Amharic voice tools')
            with contextlib.redirect_stdout(sys.stderr):
                self.model, self.stream_factory = (self.model_factory(self.bundle, task)
                    if self.model_factory is load_provider else self.model_factory(self.bundle))
                self.model_key = key
        if self.model_factory is load_provider:
            self.stream_factory = streaming_factory(self.bundle, task)
        return self.model

    def process(self, request, audio_queue=None):
        output = Path(request['output'])
        noise = self.noise_provider(request)
        if request['type'] == 'transcribe':
            self.emit(stage='Preparing recording')
            with contextlib.redirect_stdout(sys.stderr):
                audio = self.audio_loader(Path(request['audio']), noise) if noise is not None else self.audio_loader(Path(request['audio']))
            model = self.provider('transcription')
            self.emit(stage='Recognizing and timing words')
            with contextlib.redirect_stdout(sys.stderr):
                result = model.transcribe(audio)
            if result.get('model_version') != model.version:
                raise ValueError('model_version_mismatch')
            checked_words(result['words'], len(audio) / RATE)
            self.emit(stage='Saving transcript')
        else:
            model = self.provider('dictation')
            with contextlib.redirect_stdout(sys.stderr):
                if self.decode_seconds is None:
                    self.decode_seconds = self.warmup(model)
                    if not math.isfinite(self.decode_seconds) or self.decode_seconds < 0:
                        raise ValueError('invalid_readiness_measurement')
                stream = self.stream_factory(model)
                if audio_queue.rate != (48000 if noise is not None else RATE):
                    raise ValueError('invalid_sample_rate')
                preprocessor = noise.begin() if noise is not None else None
            self.emit(event='ready', model_version=model.version, decode_seconds=self.decode_seconds,
                      device=getattr(model, '_studio_device', 'cpu'),
                      dtype=getattr(model, '_studio_dtype', 'float32'))
            with contextlib.redirect_stdout(sys.stderr):
                words, count = consume(stream, audio_queue, self.emit, preprocessor)
            if count < 400:
                raise ValueError('recording_too_short')
            result = {'text': ' '.join(word['word'] for word in words), 'words': words,
                      'duration': count / RATE, 'model_version': model.version}
        annotate_model(result, {'model_name': getattr(model, '_studio_model_name', None)})
        if noise is not None:
            result['processing'] = {'noise_reduction': noise.metadata()}
        # Both modes preserve the same identity before committing the result atomically.
        atomic_result(output, result)
        self.emit(event='finished', model_version=model.version)


def serve(resident, source=None, terminate=os._exit):
    def fatal(code):
        try:
            resident.emit(error=code)
        finally:
            # A vanished parent's stdout may already be closed. It must not
            # prevent releasing a loaded model or interrupting ongoing decode.
            terminate(1)
    requests = Requests(fatal)
    reader = threading.Thread(target=read_requests, args=(requests, source or RawInput()), daemon=True)
    reader.start()
    while True:
        request, audio_queue = requests.take()
        resident.process(request, audio_queue)
        # process() has returned: live stream, audio arrays and result locals are gone.
        requests.clear()
        request = audio_queue = None
        resident.emit(event='idle')


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--bundle', type=Path, required=True)
    args = parser.parse_args()
    try:
        protect_offline()
        serve(Resident(args.bundle))
    except Exception as exc:
        if os.environ.get('ETHIOPIC_ASR_DEVELOPER_DIAGNOSTICS') == '1':
            import traceback
            traceback.print_exc(file=sys.stderr)
        event(error=error_code(exc))
        sys.exit(1)
