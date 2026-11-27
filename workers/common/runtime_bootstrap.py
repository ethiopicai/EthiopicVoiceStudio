"""Portable private-runtime bootstrap and bounded stdin for offline voice workers.

Windows DLL initialization must finish before stdin readers start. Polling the
pipe also prevents a blocked read from locking stdin during later ML DLL loads.
"""
import os
from pathlib import Path
import sys
import time

_PRIVATE_RUNTIME_HANDLES = {}
_PRIVATE_CRT = ('vcruntime140.dll', 'vcruntime140_1.dll', 'msvcp140.dll', 'msvcp140_1.dll')
_WINDOWS_STDIN_READER = None


class _WindowsStdinReader:
    """Read a pipe without holding the Windows CRT's stdin file lock."""
    def __init__(self):
        import ctypes
        from ctypes import wintypes
        import msvcrt
        self.handle = msvcrt.get_osfhandle(0)
        if self.handle in (-1, -2):
            raise OSError('Worker stdin is unavailable')
        self.library = ctypes.WinDLL('kernel32', use_last_error=True)
        self.read_file = self.library.ReadFile
        self.read_file.argtypes = [wintypes.HANDLE, wintypes.LPVOID, wintypes.DWORD,
                                   ctypes.POINTER(wintypes.DWORD), wintypes.LPVOID]
        self.read_file.restype = wintypes.BOOL
        self.peek_pipe = self.library.PeekNamedPipe
        self.peek_pipe.argtypes = [wintypes.HANDLE, wintypes.LPVOID, wintypes.DWORD,
                                  ctypes.POINTER(wintypes.DWORD), ctypes.POINTER(wintypes.DWORD),
                                  ctypes.POINTER(wintypes.DWORD)]
        self.peek_pipe.restype = wintypes.BOOL

    def read(self, size):
        import ctypes
        from ctypes import wintypes
        # Even a synchronous Win32 read can hold the stdin file-object lock
        # while a native DLL's initialization asks about stdin. Poll first so
        # this single reader never calls ReadFile on an empty pipe.
        while True:
            available = wintypes.DWORD()
            if not self.peek_pipe(self.handle, None, 0, None, ctypes.byref(available), None):
                code = ctypes.get_last_error()
                if code in (109, 232, 233):
                    return b''
                raise OSError(code, 'Worker stdin could not be read')
            if available.value:
                size = min(size, available.value)
                break
            time.sleep(.02)
        buffer = ctypes.create_string_buffer(size)
        count = wintypes.DWORD()
        if not self.read_file(self.handle, buffer, size, ctypes.byref(count), None):
            code = ctypes.get_last_error()
            if code in (109, 232, 233):  # broken pipe, no data, disconnected pipe
                return b''
            raise OSError(code, 'Worker stdin could not be read')
        return buffer.raw[:count.value]


def prepare_stdin():
    """Resolve the private process's stdin handle before starting its reader."""
    global _WINDOWS_STDIN_READER
    if sys.platform == 'win32' and _WINDOWS_STDIN_READER is None:
        _WINDOWS_STDIN_READER = _WindowsStdinReader()


def read_stdin(size):
    if type(size) is not int or not 0 < size <= 65536:
        raise ValueError('invalid_read_size')
    if sys.platform != 'win32':
        return os.read(0, size)
    prepare_stdin()
    return _WINDOWS_STDIN_READER.read(size)


def prepare_private_runtime():
    """Load this verified runtime's CRT before any ML extension is imported.

    Loading by absolute filename keeps the private runtime usable on a clean
    Windows computer without changing PATH or installing system-wide runtimes.
    Keep each returned handle alive for the lifetime of the worker.
    """
    if sys.platform != 'win32':
        return
    # Isolated Windows Python defaults redirected pipes to the local code page.
    # IPC must carry Ethiopic text as UTF-8 independently of that user setting.
    for stream in (sys.stdout, sys.stderr):
        reconfigure = getattr(stream, 'reconfigure', None)
        if reconfigure is not None:
            reconfigure(encoding='utf-8', errors='strict')
    directory = Path(sys.executable).resolve(strict=True).parent
    paths = [directory / name for name in _PRIVATE_CRT]
    if any(not path.is_file() or path.resolve(strict=True).parent != directory for path in paths):
        raise RuntimeError('runtime_incomplete')
    import ctypes
    for path in paths:
        filename = str(path)
        if filename not in _PRIVATE_RUNTIME_HANDLES:
            _PRIVATE_RUNTIME_HANDLES[filename] = ctypes.WinDLL(filename)



class ProtocolInput:
    """Bounded UTF-8 JSON framing with a single owner of the parent pipe."""
    def __init__(self):
        prepare_stdin()
        self.buffer = bytearray()

    def readline(self, limit=2 * 1024 * 1024 + 1):
        if type(limit) is not int or not 0 < limit <= 2 * 1024 * 1024 + 1:
            raise ValueError('invalid_message_limit')
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
