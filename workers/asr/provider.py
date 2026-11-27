"""Load a verified, local recognition profile without downloading code or weights.

The managed installer verifies recognition.json and every variant file before this
module is called. Legacy Mac bundles remain supported without a profile file.
"""
import functools
import gc
import importlib.util
import json
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


def selection(bundle, task='transcription'):
    if task not in ('transcription', 'dictation'):
        raise ValueError('invalid_recognition_task')
    bundle = Path(bundle)
    path = bundle / 'recognition.json'
    if not path.exists():
        return None
    if path.stat().st_size > 65536:
        raise ValueError('invalid_recognition_profile')
    value = json.loads(path.read_text(encoding='utf-8'))
    if not isinstance(value, dict) or value.get('schema_version') != 1:
        raise ValueError('invalid_recognition_profile')
    item = value.get(task)
    required = {'variant', 'backend', 'folder', 'device', 'dtype', 'model_version', 'model'}
    optional = {'threads', 'context_sec', 'update_sec'}
    if not isinstance(item, dict) or not required <= set(item) or set(item) - required - optional:
        raise ValueError('invalid_recognition_profile')
    if any(not isinstance(item[name], str) or not item[name] or len(item[name]) > 200
           or any(ord(c) < 32 for c in item[name]) for name in required):
        raise ValueError('invalid_recognition_profile')
    folder = item['folder']
    if '/' in folder or '\\' in folder or ':' in folder or folder in ('.', '..'):
        raise ValueError('invalid_recognition_profile')
    if folder != item['variant']:
        raise ValueError('invalid_recognition_profile')
    allowed = {('onnx', 'cpu', 'int8'), ('onnx', 'cpu', 'float32'),
               ('torch', 'cpu', 'float32'), ('torch', 'cuda', 'float16'),
               ('torch', 'cuda', 'float32')}
    if (item['backend'], item['device'], item['dtype']) not in allowed:
        raise ValueError('invalid_recognition_profile')
    threads = item.get('threads', min(8, os.cpu_count() or 1))
    if isinstance(threads, bool) or not isinstance(threads, int) or not 1 <= threads <= 64:
        raise ValueError('invalid_recognition_profile')
    for name, minimum, maximum in [('context_sec', 0, 10), ('update_sec', .1, 5)]:
        v = item.get(name, 2.0 if name == 'context_sec' and item['backend'] == 'onnx'
                     else 5.0 if name == 'context_sec' else .4)
        if isinstance(v, bool) or not isinstance(v, (int, float)) or not minimum <= v <= maximum:
            raise ValueError('invalid_recognition_profile')
    base = bundle.resolve(strict=True)
    actual = (bundle / folder).resolve(strict=True)
    if actual.parent != base:
        raise ValueError('invalid_recognition_profile')
    return dict(item)


def provider_key(bundle, task='transcription'):
    item = selection(bundle, task)
    if item is None:
        return 'legacy'
    # Streaming cadence/context belongs to a session, not the loaded weights.
    item.pop('context_sec', None)
    item.pop('update_sec', None)
    item.setdefault('threads', min(8, os.cpu_count() or 1))
    return json.dumps(item, sort_keys=True)


def streaming_factory(bundle, task='dictation'):
    item = selection(bundle, task)
    context = item.get('context_sec', 2. if item['backend'] == 'onnx' else 5.) if item else 5.
    update = item.get('update_sec', .4) if item else .4
    from streaming import StreamingTranscriber
    return functools.partial(StreamingTranscriber, context_sec=context, update_sec=update)


def _metadata(folder, item):
    path = folder / 'variant.json'
    metadata = json.loads(path.read_text(encoding='utf-8'))
    if metadata.get('id') != item['variant']:
        raise ValueError('model_identity_mismatch')
    if metadata.get('model_version') != item['model_version']:
        raise ValueError('model_version_mismatch')
    family = metadata.get('model')
    if family is not None and family != item['model']:
        raise ValueError('model_identity_mismatch')
    return metadata


def _local_module(folder, name):
    path = folder / (name + '.py')
    spec = importlib.util.spec_from_file_location('_studio_' + name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def _load_torch(folder, device, dtype, threads):
    import torch
    torch.set_num_threads(threads)
    if device == 'cuda' and not torch.cuda.is_available():
        raise ValueError('gpu_unavailable')
    if device == 'cuda' and dtype == 'float16' and torch.cuda.get_device_capability()[0:2] < (7, 5):
        raise ValueError('gpu_precision_unsupported')
    # A variant's package is part of the verified download, never a hub import.
    sys.path.insert(0, str(folder))
    for name in list(sys.modules):
        if name == 'omniasr_hf' or name.startswith('omniasr_hf.'):
            del sys.modules[name]
    from omniasr_hf.transcribe import CTCTranscriber
    return CTCTranscriber(str(folder / 'model'), device=device, dtype=getattr(torch, dtype))


def load_provider(bundle, task='transcription'):
    prepare_private_runtime()
    bundle = Path(bundle)
    item = selection(bundle, task)
    if item is None:
        model = _load_torch(bundle, 'cpu', 'float32', min(8, os.cpu_count() or 1))
        metadata = json.loads((bundle / 'model/version.json').read_text(encoding='utf-8'))
        expected, family = metadata['model_version'], metadata.get('model_name')
        release_file=bundle/'release.json'
        if release_file.is_file():
            release=json.loads(release_file.read_text(encoding='utf-8'))
            if (release.get('schema_version')==1 and release.get('variant')=='asr-wav2vec_ctc-1b-amh-fp16'
                    and release.get('weight_version')==expected=='v0.3' and release.get('model_version')=='v1.0'):
                expected=release['model_version']
        device, dtype = 'cpu', 'float32'
    else:
        folder = bundle / item['folder']
        _metadata(folder, item)
        threads = item.get('threads', min(8, os.cpu_count() or 1))
        if item['backend'] == 'onnx':
            import onnxruntime as ort
            ort.disable_telemetry_events()
            module = _local_module(folder, 'asr_onnx_infer')
            model = module.OnnxCTCTranscriber(str(folder), threads=threads,
                                            providers=['CPUExecutionProvider'], mem_arena=False)
        else:
            model = _load_torch(folder, item['device'], item['dtype'], threads)
        expected, family = item['model_version'], item['model']
        device, dtype = item['device'], item['dtype']
    # Manifest v2 renames the unchanged v0.3 ASR weights to release v1.0.
    # The downloaded, verified weight-side version file intentionally remains unchanged.
    if expected == 'v1.0' and getattr(model, 'version', None) == 'v0.3':
        model.version = expected
    if not getattr(model, 'version', None) or model.version != expected:
        raise ValueError('model_version_mismatch')
    model._studio_model_name = family
    model._studio_device = device
    model._studio_dtype = dtype
    return model, streaming_factory(bundle, task)


def release_cache(device):
    """Call after dropping model references; no Torch import for an ONNX runtime."""
    gc.collect()
    if device == 'cuda' and 'torch' in sys.modules:
        sys.modules['torch'].cuda.empty_cache()
