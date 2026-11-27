"""Offline activation check for both selected recognition providers and streaming."""
import argparse
import contextlib
import os
from pathlib import Path
import sys

from dictation_worker import after_boundary, checked_words
from provider import load_provider, provider_key, release_cache, streaming_factory, read_stdin
from resident_worker import atomic_result, error_code, protect_offline
from worker import annotate_model, load_audio


def run(bundle, audio, output):
    protect_offline()
    with contextlib.redirect_stdout(sys.stderr):
        samples = load_audio(audio)
        batch, stream_factory = load_provider(bundle, 'transcription')
        result = batch.transcribe(samples)
        if result.get('model_version') != batch.version:
            raise ValueError('model_version_mismatch')
        checked_words(result['words'], len(samples) / 16000)
        annotate_model(result, {'model_name': batch._studio_model_name})
        if provider_key(bundle, 'transcription') != provider_key(bundle, 'dictation'):
            device = batch._studio_device
            del batch
            release_cache(device)
            live, stream_factory = load_provider(bundle, 'dictation')
        else:
            live = batch
        stream_factory = streaming_factory(bundle, 'dictation')
        stream = stream_factory(live)
        update = stream.feed(samples)
        final = [] if update is None else checked_words(update['final'], len(samples) / 16000)
        boundary = final[-1]['end'] if final else 0
        final += after_boundary(checked_words(stream.finish()['final'], len(samples) / 16000), boundary)
        checked_words(final, len(samples) / 16000)
        result.update(dictation_model_version=live.version,
                      dictation_model=live._studio_model_name, streaming_checked=True)
    atomic_result(output, result)


def main(argv=None):
    parser = argparse.ArgumentParser()
    parser.add_argument('--bundle', type=Path, required=True)
    parser.add_argument('--audio', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args(argv)
    try:
        # Load Windows CRT dependencies before a thread blocks in CRT stdin.
        protect_offline()
        if os.environ.get('ETHIOPIC_MANAGED_WORKER') == '1':
            import threading
            def watch_parent():
                try:
                    while read_stdin(1):
                        pass
                finally:
                    os._exit(1)
            threading.Thread(target=watch_parent, daemon=True).start()
        run(args.bundle, args.audio, args.output)
    except Exception as exc:
        atomic_result(args.output, {'error': error_code(exc)})
        return 1
    return 0


if __name__ == '__main__':
    sys.exit(main())
