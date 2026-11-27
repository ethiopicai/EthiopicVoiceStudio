import json
import ctypes
import io
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock

import numpy as np

import provider
from streaming import StreamingTranscriber


def descriptor(variant='asr-wav2vec_ctc-300m-amh-int8_onnx'):
    return {'variant': variant, 'backend': 'onnx', 'folder': variant,
            'device': 'cpu', 'dtype': 'int8', 'model_version': 'v0.3',
            'model': 'amharic-ctc-300m', 'threads': 4, 'context_sec': 2}


class ProviderTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.bundle = Path(self.directory.name)
        bootstrap = mock.patch.object(provider, 'prepare_private_runtime')
        bootstrap.start()
        self.addCleanup(bootstrap.stop)

    def profile(self, batch=None, live=None):
        batch, live = batch or descriptor(), live or batch or descriptor()
        for item in [batch, live]:
            folder = self.bundle / item['folder']
            folder.mkdir(exist_ok=True)
            (folder / 'variant.json').write_text(json.dumps({
                'id': item['variant'], 'model': item['model'],
                'model_version': item['model_version']}))
        (self.bundle / 'recognition.json').write_text(json.dumps({
            'schema_version': 1, 'transcription': batch, 'dictation': live}))

    def test_onnx_loader_never_imports_torch_and_preserves_model_identity(self):
        self.profile()
        calls = []
        model = SimpleNamespace(version='v0.3')
        module = SimpleNamespace(OnnxCTCTranscriber=lambda *args, **kwargs:
                                 calls.append((args, kwargs)) or model)
        ort = SimpleNamespace(disable_telemetry_events=lambda: calls.append('telemetry-disabled'))
        original_import = __import__
        def no_torch(name, *args, **kwargs):
            if name == 'torch' or name.startswith('torch.'):
                self.fail('The ONNX recognition provider imported Torch')
            return original_import(name, *args, **kwargs)
        with mock.patch.dict('sys.modules', {'onnxruntime': ort}), \
             mock.patch('builtins.__import__', side_effect=no_torch), \
             mock.patch.object(provider, '_local_module', return_value=module):
            loaded, stream_factory = provider.load_provider(self.bundle, 'dictation')
        self.assertIs(loaded, model)
        self.assertEqual(loaded._studio_model_name, 'amharic-ctc-300m')
        self.assertEqual(loaded._studio_dtype, 'int8')
        self.assertEqual(stream_factory(loaded).context, 2)
        self.assertEqual(calls[0], 'telemetry-disabled')
        self.assertEqual(calls[1][1]['providers'], ['CPUExecutionProvider'])

    def test_batch_and_live_route_separately_but_identical_gpu_profile_reuses_key(self):
        batch = descriptor('asr-wav2vec_ctc-1b-amh-int8_onnx')
        batch['model'] = 'amharic-ctc'
        self.profile(batch, descriptor())
        self.assertNotEqual(provider.provider_key(self.bundle, 'transcription'),
                            provider.provider_key(self.bundle, 'dictation'))
        self.profile(batch, batch)
        self.assertEqual(provider.provider_key(self.bundle, 'transcription'),
                         provider.provider_key(self.bundle, 'dictation'))
        live = {**batch, 'context_sec': 3, 'update_sec': 1.2}
        self.profile(batch, live)
        self.assertEqual(provider.provider_key(self.bundle, 'transcription'),
                         provider.provider_key(self.bundle, 'dictation'))
        stream = provider.streaming_factory(self.bundle, 'dictation')(None)
        self.assertEqual(stream.context, 3)
        self.assertEqual(stream.update, 19200)

    def test_profile_rejects_traversal_wrong_device_and_invalid_runtime_parameters(self):
        self.profile()
        path = self.bundle / 'recognition.json'
        valid = json.loads(path.read_text())
        for field, value in [('folder', '../outside'), ('device', 'cuda'),
                             ('threads', True), ('threads', 0), ('context_sec', float('nan')),
                             ('dtype', 'float16'), ('update_sec', 0), ('backend', 'remote')]:
            profile = json.loads(json.dumps(valid))
            profile['transcription'][field] = value
            path.write_text(json.dumps(profile))
            with self.subTest(field=field, value=value), self.assertRaises(ValueError):
                provider.selection(self.bundle)

    def test_metadata_version_and_family_cannot_silently_change(self):
        self.profile()
        folder = self.bundle / descriptor()['folder']
        for changed in [{'model_version': 'v9'}, {'model': 'wrong'}, {'id': 'wrong'}]:
            metadata = {**descriptor(), **changed}
            metadata['id'] = changed.get('id', descriptor()['variant'])
            (folder / 'variant.json').write_text(json.dumps(metadata))
            with self.subTest(changed=changed), self.assertRaises(ValueError):
                provider.load_provider(self.bundle)

    def test_naming_release_wraps_unchanged_weight_version_but_rejects_other_weights(self):
        item={**descriptor(), 'model_version':'v1.0'}
        self.profile(item,item)
        ort=SimpleNamespace(disable_telemetry_events=lambda:None)
        for engine_version in ['v0.3','v0.2']:
            model=SimpleNamespace(version=engine_version)
            module=SimpleNamespace(OnnxCTCTranscriber=lambda *args,**kwargs:model)
            with mock.patch.dict('sys.modules',{'onnxruntime':ort}), mock.patch.object(provider,'_local_module',return_value=module):
                if engine_version=='v0.3':
                    loaded,_=provider.load_provider(self.bundle);self.assertEqual(loaded.version,'v1.0')
                else:
                    with self.assertRaisesRegex(ValueError,'model_version_mismatch'):provider.load_provider(self.bundle)

    def test_legacy_mac_bundle_keeps_cpu_float32_loading(self):
        (self.bundle / 'model').mkdir()
        (self.bundle / 'model/version.json').write_text(json.dumps({
            'model_version': 'legacy-v1', 'model_name': 'amharic-ctc'}))
        model = SimpleNamespace(version='legacy-v1')
        with mock.patch.object(provider, '_load_torch', return_value=model) as loader:
            loaded, _ = provider.load_provider(self.bundle)
        self.assertIs(loaded, model)
        self.assertEqual(loader.call_args.args[1:3], ('cpu', 'float32'))
        self.assertEqual(loaded._studio_model_name, 'amharic-ctc')

    def test_resident_switches_batch_and_live_families_without_reloading_same_mode(self):
        from resident_worker import Resident
        from dictation_worker import AudioQueue
        batch = descriptor('asr-wav2vec_ctc-1b-amh-int8_onnx')
        batch['model'] = 'amharic-ctc'
        self.profile(batch, descriptor())
        loaded = []
        class Model:
            version = 'v0.3'
            def transcribe(self, audio):
                return {'text': '', 'words': [], 'duration': len(audio) / 16000,
                        'model_version': self.version}
            def decode(self, audio, offset=0):
                return []
        def make_model(folder, **kwargs):
            loaded.append(folder)
            return Model()
        ort = SimpleNamespace(disable_telemetry_events=lambda: None)
        module = SimpleNamespace(OnnxCTCTranscriber=make_model)
        with mock.patch.dict('sys.modules', {'onnxruntime': ort}), \
             mock.patch.object(provider, '_local_module', return_value=module):
            resident = Resident(self.bundle, emit=lambda **event: None,
                                warmup=lambda model: .1, audio_loader=lambda path: np.zeros(800, np.float32))
            output = self.bundle / 'batch.json'
            for _ in range(2):
                resident.process({'type': 'transcribe', 'audio': 'audio.wav', 'output': str(output)})
            self.assertEqual(len(loaded), 1)
            self.assertEqual(json.loads(output.read_text())['model'], 'amharic-ctc')
            queue = AudioQueue()
            queue.push([0.] * 800)
            queue.finish()
            live_output = self.bundle / 'live.json'
            resident.process({'type': 'dictation', 'output': str(live_output)}, queue)
            self.assertEqual(len(loaded), 2)
            self.assertEqual(json.loads(live_output.read_text())['model'], 'amharic-ctc-300m')

    def test_activation_failures_return_safe_stable_codes_without_private_details(self):
        import setup_check
        output = self.bundle / 'result.json'
        for exc, expected in [(ValueError('gpu_unavailable'), 'gpu_unavailable'),
                              (ValueError('gpu_precision_unsupported'), 'gpu_precision_unsupported'),
                              (MemoryError('private information'), 'out_of_memory'),
                              (RuntimeError('private model/audio path'), 'worker_failed')]:
            with self.subTest(expected=expected), mock.patch.object(setup_check, 'run', side_effect=exc), \
                 mock.patch.object(setup_check, 'protect_offline'):
                code = setup_check.main(['--bundle', str(self.bundle), '--audio', 'test.wav',
                                         '--output', str(output)])
                self.assertEqual(code, 1)
                self.assertEqual(json.loads(output.read_text()), {'error': expected})

    def test_activation_bootstraps_before_parent_watch_thread(self):
        import setup_check
        order = []
        watcher = SimpleNamespace(start=lambda: order.append('watch-parent'))
        with mock.patch.dict('os.environ', {'ETHIOPIC_MANAGED_WORKER': '1'}), \
             mock.patch.object(setup_check, 'protect_offline', side_effect=lambda: order.append('bootstrap')), \
             mock.patch('threading.Thread', return_value=watcher), \
             mock.patch.object(setup_check, 'run', side_effect=lambda *args: order.append('inference')):
            result = setup_check.main(['--bundle', str(self.bundle), '--audio', 'test.wav',
                                       '--output', str(self.bundle / 'result.json')])
        self.assertEqual(result, 0)
        self.assertEqual(order, ['bootstrap', 'watch-parent', 'inference'])


class PrivateRuntimeTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.python = self.root / 'python.exe'
        self.python.write_bytes(b'private interpreter')
        for name in provider._PRIVATE_CRT:
            (self.root / name).write_bytes(b'verified runtime library')

    def test_windows_loads_only_absolute_private_libraries_and_retains_handles(self):
        handles = [object() for _ in provider._PRIVATE_CRT]
        with mock.patch.object(provider.sys, 'platform', 'win32'), \
             mock.patch.object(provider.sys, 'executable', str(self.python)), \
             mock.patch.dict(provider._PRIVATE_RUNTIME_HANDLES, clear=True), \
             mock.patch.object(ctypes, 'WinDLL', create=True, side_effect=handles) as loader:
            provider.prepare_private_runtime()
            provider.prepare_private_runtime()
            self.assertEqual(loader.call_count, 4)
            self.assertEqual([call.args[0] for call in loader.call_args_list],
                             [str(self.root.resolve() / name) for name in provider._PRIVATE_CRT])
            self.assertEqual(list(provider._PRIVATE_RUNTIME_HANDLES.values()), handles)

    def test_missing_private_library_fails_without_system_search(self):
        (self.root / 'msvcp140.dll').unlink()
        with mock.patch.object(provider.sys, 'platform', 'win32'), \
             mock.patch.object(provider.sys, 'executable', str(self.python)), \
             mock.patch.object(ctypes, 'WinDLL', create=True) as loader:
            with self.assertRaisesRegex(RuntimeError, 'runtime_incomplete'):
                provider.prepare_private_runtime()
            loader.assert_not_called()

    def test_mac_does_not_load_windows_libraries(self):
        with mock.patch.object(provider.sys, 'platform', 'darwin'), \
             mock.patch.object(ctypes, 'WinDLL', create=True) as loader:
            provider.prepare_private_runtime()
            loader.assert_not_called()

    def test_windows_unicode_ipc_uses_utf8_even_when_python_started_with_cp1252(self):
        import resident_worker
        raw = io.BytesIO()
        output = io.TextIOWrapper(raw, encoding='cp1252', write_through=True)
        self.addCleanup(output.close)
        with mock.patch.object(provider.sys, 'platform', 'win32'), \
             mock.patch.object(provider.sys, 'executable', str(self.python)), \
             mock.patch.object(provider.sys, 'stdout', output), \
             mock.patch.object(resident_worker, 'PROTOCOL_OUT', output), \
             mock.patch.dict(provider._PRIVATE_RUNTIME_HANDLES, clear=True), \
             mock.patch.object(ctypes, 'WinDLL', create=True):
            provider.prepare_private_runtime()
            resident_worker.event(event='transcript', final=[{'word': 'ሰላም', 'start': 0, 'end': 1}], partial=[])
            output.flush()
        decoded = json.loads(raw.getvalue().decode('utf-8'))
        self.assertEqual(decoded['final'][0]['word'], 'ሰላም')
        self.assertIn('ሰላም'.encode('utf-8'), raw.getvalue())


class InputTests(unittest.TestCase):
    def test_windows_reads_pipe_bytes_without_crt_read_and_resolves_handle_once(self):
        calls = []
        def read_file(handle, buffer, size, count, overlapped):
            calls.append((handle, size, overlapped))
            ctypes.memmove(buffer, b'\xe1\x88\xb0\n', 4)
            count._obj.value = 4
            return True
        def peek_pipe(handle, buffer, size, count, available, remaining):
            available._obj.value = 4
            return True
        library = SimpleNamespace(ReadFile=read_file, PeekNamedPipe=peek_pipe)
        msvcrt = SimpleNamespace(get_osfhandle=mock.Mock(return_value=141))
        with mock.patch.object(provider.sys, 'platform', 'win32'), \
             mock.patch.dict('sys.modules', {'msvcrt': msvcrt}), \
             mock.patch.object(provider, '_WINDOWS_STDIN_READER', None), \
             mock.patch.object(ctypes, 'WinDLL', create=True, return_value=library), \
             mock.patch.object(provider.os, 'read') as crt_read:
            provider.prepare_stdin()
            self.assertEqual(provider.read_stdin(65536), 'ሰ\n'.encode('utf-8'))
            self.assertEqual(provider.read_stdin(16), 'ሰ\n'.encode('utf-8'))
            msvcrt.get_osfhandle.assert_called_once_with(0)
            crt_read.assert_not_called()
            self.assertIs(provider._WINDOWS_STDIN_READER.library, library)
        self.assertEqual(calls, [(141, 4, None), (141, 4, None)])

    def test_windows_closed_pipes_are_eof_but_other_errors_are_not_hidden(self):
        library = SimpleNamespace(ReadFile=lambda *args: False, PeekNamedPipe=lambda *args: False)
        with mock.patch.dict('sys.modules', {'msvcrt': SimpleNamespace(get_osfhandle=lambda fd: 141)}), \
             mock.patch.object(ctypes, 'WinDLL', create=True, return_value=library), \
             mock.patch.object(ctypes, 'get_last_error', create=True) as error:
            reader = provider._WindowsStdinReader()
            for code in [109, 232, 233]:
                error.return_value = code
                self.assertEqual(reader.read(16), b'')
            error.return_value = 5
            with self.assertRaises(OSError) as exc:
                reader.read(16)
            self.assertEqual(exc.exception.errno, 5)

    def test_empty_windows_pipe_polls_without_blocking_readfile(self):
        waiting = [0, 0, 2]
        calls = []
        def peek_pipe(handle, buffer, size, count, available, remaining):
            available._obj.value = waiting.pop(0)
            return True
        def read_file(handle, buffer, size, count, overlapped):
            self.assertEqual(waiting, [])
            calls.append(size)
            ctypes.memmove(buffer, b'ok', 2)
            count._obj.value = 2
            return True
        library = SimpleNamespace(ReadFile=read_file, PeekNamedPipe=peek_pipe)
        with mock.patch.dict('sys.modules', {'msvcrt': SimpleNamespace(get_osfhandle=lambda fd: 141)}), \
             mock.patch.object(ctypes, 'WinDLL', create=True, return_value=library), \
             mock.patch.object(provider.time, 'sleep') as sleep:
            reader = provider._WindowsStdinReader()
            self.assertEqual(reader.read(65536), b'ok')
            self.assertEqual(sleep.call_args_list, [mock.call(.02), mock.call(.02)])
        self.assertEqual(calls, [2])

    def test_pipe_closed_between_peek_and_read_is_eof(self):
        def peek_pipe(handle, buffer, size, count, available, remaining):
            available._obj.value = 2
            return True
        library = SimpleNamespace(ReadFile=lambda *args: False, PeekNamedPipe=peek_pipe)
        with mock.patch.dict('sys.modules', {'msvcrt': SimpleNamespace(get_osfhandle=lambda fd: 141)}), \
             mock.patch.object(ctypes, 'WinDLL', create=True, return_value=library), \
             mock.patch.object(ctypes, 'get_last_error', create=True, return_value=109):
            self.assertEqual(provider._WindowsStdinReader().read(16), b'')

    def test_posix_uses_existing_stdin_read_and_all_platforms_bound_allocation(self):
        with mock.patch.object(provider.sys, 'platform', 'darwin'), \
             mock.patch.object(provider.os, 'read', return_value=b'line\n') as read:
            self.assertEqual(provider.read_stdin(16), b'line\n')
            read.assert_called_once_with(0, 16)
            for size in [0, -1, 65537, True]:
                with self.assertRaises(ValueError):
                    provider.read_stdin(size)


class StreamingTests(unittest.TestCase):
    def test_finished_words_never_reappear_when_context_is_decoded_again(self):
        class Model:
            calls = 0
            def decode(self, audio, offset=0):
                self.calls += 1
                return [{'word': 'first' if self.calls == 1 else 'changed', 'start': .1, 'end': .3}]
        stream = StreamingTranscriber(Model(), context_sec=2)
        first = stream.feed(np.zeros(16000, np.float32))
        frozen = dict(first['final'][0])
        second = stream.feed(np.zeros(16000, np.float32))
        tail = stream.finish()
        self.assertEqual(first['final'], [frozen])
        self.assertEqual(second['final'], [])
        self.assertEqual(tail, {'final': [], 'partial': []})
        self.assertEqual(stream.done, 0)
        self.assertEqual(len(stream.buf), 0)

    def test_partial_words_are_replaced_and_finish_commits_latest_tail(self):
        class Model:
            calls = 0
            def decode(self, audio, offset=0):
                self.calls += 1
                return [{'word': 'draft' if self.calls == 1 else 'corrected',
                         'start': .1, 'end': len(audio) / 16000}]
        stream = StreamingTranscriber(Model())
        first = stream.feed(np.zeros(6400, np.float32))
        self.assertEqual(first['final'], [])
        self.assertEqual(first['partial'][0]['word'], 'draft')
        self.assertEqual(stream.finish()['final'][0]['word'], 'corrected')

    def test_large_input_backlog_is_joined_into_one_decode(self):
        class Model:
            calls = 0
            def decode(self, audio, offset=0):
                self.calls += 1
                return []
        model = Model()
        stream = StreamingTranscriber(model, context_sec=2)
        self.assertIsNone(stream.feed(np.zeros(1600, np.float32)))
        stream.feed(np.zeros(16000 * 8, np.float32))
        self.assertEqual(model.calls, 1)
        self.assertLessEqual(len(stream.buf), 16000 * 3)


if __name__ == '__main__':
    unittest.main()
