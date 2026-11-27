import io
import json
from pathlib import Path
import queue
import subprocess
import sys
import tempfile
import threading
import time
import unittest

from dictation_worker import AudioQueue
from resident_worker import Resident, Requests, read_requests, serve


class FakeModel:
    version = 'test-model-v1'

    def transcribe(self, audio):
        return {'text': 'batch', 'words': [{'word': 'batch', 'start': 0, 'end': .01}],
                'duration': len(audio) / 16000, 'model_version': self.version}


class FakeStream:
    def __init__(self, model):
        self.model = model
        self.count = 0

    def feed(self, audio):
        self.count += len(audio)
        return {'final': [], 'partial': [{'word': 'draft', 'start': 0, 'end': .01}]}

    def finish(self):
        return {'final': [{'word': 'live', 'start': 0, 'end': self.count / 16000}], 'partial': []}


class ControlledInput:
    def __init__(self):
        self.lines = queue.Queue()

    def put(self, value):
        self.lines.put(json.dumps(value).encode() + b'\n')

    def readline(self, limit):
        return self.lines.get(timeout=10)


class ResidentTests(unittest.TestCase):
    def test_one_model_for_two_batch_jobs_and_fresh_live_sessions_idle_clears_state(self):
        with tempfile.TemporaryDirectory() as directory:
            events, loads, streams, warmups, terminated, failures = [], [], [], [], [], []
            condition = threading.Condition()
            source = ControlledInput()
            def emit(**value):
                with condition:
                    events.append(value)
                    condition.notify_all()
            def stream_factory(model):
                stream = FakeStream(model)
                streams.append(stream)
                return stream
            def factory(bundle):
                model = FakeModel()
                loads.append(model)
                return model, stream_factory
            resident = Resident(directory, emit, factory,
                                lambda model: warmups.append(model) or .125,
                                lambda path: [0.] * 800)
            def run():
                try:
                    serve(resident, source, terminated.append)
                except RuntimeError as error:
                    failures.append(str(error))
            thread = threading.Thread(target=run, daemon=True)
            thread.start()
            def wait_idle(count):
                with condition:
                    self.assertTrue(condition.wait_for(lambda: sum(event.get('event') == 'idle' for event in events) >= count, timeout=5))
            for index in range(2):
                output = str(Path(directory) / f'batch-{index}.json')
                source.put({'type': 'transcribe', 'audio': 'managed.wav', 'output': output})
                wait_idle(index + 1)
                self.assertEqual(json.loads(Path(output).read_text())['text'], 'batch')
            for index in range(2):
                output = str(Path(directory) / f'live-{index}.json')
                source.put({'type': 'dictation', 'output': output})
                source.put({'type': 'audio', 'samples': [.1] * 800})
                source.put({'type': 'finish'})
                wait_idle(index + 3)
                self.assertEqual(json.loads(Path(output).read_text())['duration'], .05)
            self.assertEqual(len(loads), 1)
            self.assertEqual(len(streams), 2)
            self.assertIsNot(streams[0], streams[1])
            self.assertTrue(all(stream.model is loads[0] for stream in streams))
            self.assertEqual([stream.count for stream in streams], [800, 800])
            self.assertEqual(len(warmups), 1)
            self.assertEqual([value['decode_seconds'] for value in events if value.get('event') == 'ready'], [.125, .125])
            lifecycle = [value['event'] for value in events if value.get('event') in {'finished', 'idle'}]
            self.assertEqual(lifecycle, ['finished', 'idle'] * 4)
            source.lines.put(b'')
            thread.join(timeout=2)
            self.assertFalse(thread.is_alive())
            self.assertEqual(terminated, [1])
            self.assertEqual(failures, ['parent_closed'])

    def test_clear_releases_request_and_audio_before_next_request(self):
        requests = Requests(lambda code: None)
        requests.accept({'type': 'dictation', 'output': 'first.json'})
        request, audio = requests.take()
        audio.push([0.] * 400)
        requests.accept({'type': 'finish'})
        requests.clear()
        self.assertIsNone(requests.request)
        self.assertIsNone(requests.audio)
        requests.accept({'type': 'dictation', 'output': 'second.json'})
        second, fresh_audio = requests.take()
        self.assertIsNot(fresh_audio, audio)
        self.assertEqual(fresh_audio.total, 0)
        self.assertFalse(fresh_audio.finished)

    def test_invalid_sequence_never_leaks_into_next_session(self):
        for message in [{'type': 'audio', 'samples': [0.]}, {'type': 'finish'},
                        {'type': 'dictation', 'output': 'ok', 'unknown': True},
                        {'type': 'transcribe', 'audio': 10, 'output': 'ok'}]:
            with self.assertRaises(ValueError):
                Requests(lambda code: None).accept(message)
        requests = Requests(lambda code: None)
        requests.accept({'type': 'dictation', 'output': 'first'})
        requests.accept({'type': 'finish'})
        with self.assertRaises(ValueError):
            requests.accept({'type': 'audio', 'samples': [0.]})
        with self.assertRaises(ValueError):
            requests.accept({'type': 'transcribe', 'audio': 'input', 'output': 'next'})

    def test_reader_rejects_partial_framing_and_parent_eof(self):
        for raw, expected in [(b'', 'parent_closed'), (b'{"type":"finish"}', 'worker_failed'),
                              (b'[]\n', 'worker_failed')]:
            failures = []
            requests = Requests(failures.append)
            read_requests(requests, io.BytesIO(raw))
            self.assertEqual(failures, [expected])
            with self.assertRaisesRegex(RuntimeError, expected):
                requests.take()

    def test_eof_terminates_process_during_inference_not_after_slow_decode(self):
        code = r'''
import time
from resident_worker import Resident, serve
class Model:
    version='v1'
    def transcribe(self,audio):
        time.sleep(60)
def factory(bundle): return Model(),None
serve(Resident('.', model_factory=factory, audio_loader=lambda path:[0.]*800))
'''
        child = subprocess.Popen([sys.executable, '-u', '-c', code], cwd=Path(__file__).parent,
                                 stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        try:
            child.stdin.write(b'{"type":"transcribe","audio":"input.wav","output":"result.json"}\n')
            child.stdin.flush()
            for _ in range(4):
                line = json.loads(child.stdout.readline())
                if line.get('stage') == 'Recognizing and timing words':
                    break
            else:
                self.fail('Worker did not enter recognition')
            started = time.monotonic()
            child.stdin.close()
            self.assertEqual(child.wait(timeout=3), 1)
            self.assertLess(time.monotonic() - started, 3)
            self.assertIn(b'parent_closed', child.stdout.read())
        finally:
            if child.poll() is None:
                child.kill()
                child.wait(timeout=3)
            child.stdout.close()
            child.stderr.close()

    def test_version_mismatch_does_not_publish_batch_output(self):
        class BadModel(FakeModel):
            def transcribe(self, audio):
                return {**super().transcribe(audio), 'model_version': 'wrong'}
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / 'output.json'
            resident = Resident(directory, lambda **event: None, lambda bundle: (BadModel(), None),
                                audio_loader=lambda path: [0.] * 800)
            with self.assertRaisesRegex(ValueError, 'model_version_mismatch'):
                resident.process({'type': 'transcribe', 'audio': 'input.wav', 'output': str(output)})
            self.assertFalse(output.exists())


if __name__ == '__main__':
    unittest.main()
