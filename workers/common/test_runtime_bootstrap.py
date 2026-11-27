import ctypes
import json
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest import mock

import runtime_bootstrap as runtime


class InputTests(unittest.TestCase):
    def test_fragmented_amharic_json_round_trips_without_unbounded_buffering(self):
        message = json.dumps({'text': 'ሰላም'}, ensure_ascii=False).encode('utf-8') + b'\n'
        pieces = [message[:4], message[4:13], message[13:], b'']
        with mock.patch.object(runtime, 'prepare_stdin'), \
             mock.patch.object(runtime, 'read_stdin', side_effect=pieces):
            source = runtime.ProtocolInput()
            self.assertEqual(json.loads(source.readline()), {'text': 'ሰላም'})
            self.assertEqual(source.readline(), b'')

    def test_oversized_and_partial_frames_are_returned_for_caller_to_reject(self):
        with mock.patch.object(runtime, 'prepare_stdin'), \
             mock.patch.object(runtime, 'read_stdin', side_effect=[b'12345', b'']):
            source = runtime.ProtocolInput()
            self.assertEqual(source.readline(5), b'12345')
            self.assertEqual(source.readline(5), b'')
            with self.assertRaises(ValueError):
                source.readline(0)

    def test_windows_reader_never_blocks_readfile_on_an_empty_pipe(self):
        remaining = [0, 2]
        def peek(handle, buffer, size, count, available, left):
            available._obj.value = remaining.pop(0)
            return True
        def read(handle, buffer, size, count, overlapped):
            self.assertEqual(remaining, [])
            self.assertEqual(size, 2)
            ctypes.memmove(buffer, b'ok', 2)
            count._obj.value = 2
            return True
        library = SimpleNamespace(ReadFile=read, PeekNamedPipe=peek)
        with mock.patch.dict('sys.modules', {'msvcrt': SimpleNamespace(get_osfhandle=lambda fd: 141)}), \
             mock.patch.object(ctypes, 'WinDLL', create=True, return_value=library), \
             mock.patch.object(runtime.time, 'sleep') as sleep:
            self.assertEqual(runtime._WindowsStdinReader().read(65536), b'ok')
            sleep.assert_called_once_with(.02)

    def test_private_crt_is_loaded_by_absolute_path_and_handles_are_retained(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            python = root / 'python.exe'
            python.write_bytes(b'private interpreter')
            for name in runtime._PRIVATE_CRT:
                (root / name).write_bytes(b'pinned DLL')
            handles = [object() for _ in runtime._PRIVATE_CRT]
            with mock.patch.object(runtime.sys, 'platform', 'win32'), \
                 mock.patch.object(runtime.sys, 'executable', str(python)), \
                 mock.patch.dict(runtime._PRIVATE_RUNTIME_HANDLES, clear=True), \
                 mock.patch.object(ctypes, 'WinDLL', create=True, side_effect=handles) as load:
                runtime.prepare_private_runtime()
                runtime.prepare_private_runtime()
                self.assertEqual(load.call_count, 4)
                self.assertEqual([call.args[0] for call in load.call_args_list],
                                 [str(root.resolve() / name) for name in runtime._PRIVATE_CRT])
                self.assertEqual(list(runtime._PRIVATE_RUNTIME_HANDLES.values()), handles)


if __name__ == '__main__':
    unittest.main()
