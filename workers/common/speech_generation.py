"""Validated generation controls shared by the offline speech providers."""
import math
import re
from pathlib import Path

MAX_SECONDS = 600
_SENTENCE_END = re.compile(r'(?<=[\u1362\u1367\u1368?!.\u2026])\s+')


def split_sentences(text):
    return [part for part in (value.strip() for value in _SENTENCE_END.split(text.strip()))
            if any(char.isalnum() for char in part)]


def _number(value, low, high):
    if isinstance(value, bool) or not isinstance(value, (int, float)) or not math.isfinite(value) or not low <= value <= high:
        raise ValueError('Invalid speech settings')
    return float(value)


def validate_request(request, engine, default_steps=32):
    if engine not in ('cpu', 'omnivoice'):
        raise ValueError('Unsupported speech engine')
    seed = request.get('seed', 0)
    if type(seed) is not int or not 0 <= seed <= 4294967295:
        raise ValueError('Invalid speech seed')
    raw = request.get('generationOptions')
    options = {} if raw is None else raw
    allowed = {'temperature', 'sentencePause'}
    if engine == 'omnivoice':
        allowed |= {'speed', 'duration', 'numSteps'}
    if not isinstance(options, dict) or set(options) - allowed:
        raise ValueError('Invalid speech settings')
    result = {'seed': seed, 'legacy': raw is None}
    if 'temperature' in options:
        result['temperature'] = _number(options['temperature'], .1 if engine == 'cpu' else 0., 1.)
    elif engine == 'omnivoice':
        result['temperature'] = 0.
    else:
        result['temperature'] = None
    if engine == 'cpu':
        result['sentencePause'] = _number(options.get('sentencePause', .2), 0., 2.)
        return result
    steps = options.get('numSteps', default_steps)
    if type(steps) is not int or steps not in (16, 32, 64):
        raise ValueError('Choose 16, 32 or 64 generation steps')
    result['numSteps'] = steps
    if 'duration' in options:
        if 'speed' in options or 'sentencePause' in options:
            raise ValueError('Use target duration with manual speed and sentence pause disabled')
        result['duration'] = _number(options['duration'], .5, MAX_SECONDS)
    else:
        result['speed'] = _number(options.get('speed', 1.), .5, 2.)
        if 'sentencePause' in options:
            result['sentencePause'] = _number(options['sentencePause'], 0., 2.)
    return result


def checked_audio(value, rate, frames=0):
    import numpy as np
    value = np.asarray(value, dtype=np.float32)
    if value.ndim != 1 or not value.size or not np.isfinite(value).all():
        raise ValueError('Invalid generated audio')
    if frames + len(value) > rate * MAX_SECONDS:
        raise ValueError('Generated audio is too long')
    return value


def write_wav_atomic(path, value, rate):
    import soundfile as sf
    value = checked_audio(value, rate)
    path = Path(path)
    pending = path.with_suffix('.tmp')
    if path.is_symlink() or pending.is_symlink():
        raise ValueError('Redirected output')
    try:
        sf.write(pending, value, rate, format='WAV', subtype='PCM_16')
        pending.replace(path)
    finally:
        if pending.is_file() and not pending.is_symlink():
            pending.unlink()


def synthesize_omnivoice(tts, text, voice, controls, rate=24000):
    """Forward model controls; insert only explicitly requested sentence silence.

    Target duration stays a model request, never a resampling/stretching pass.
    The SDK may quantize to codec frames or chunk long scripts internally.
    """
    import numpy as np
    seed, steps = controls['seed'], controls['numSteps']
    settings = {'schemaVersion': 1, 'seed': seed, 'temperature': controls['temperature'], 'numSteps': steps}
    if controls['legacy']:
        value = checked_audio(tts.synth(text, voice, num_step=steps, seed=seed), rate)
        settings['speed'] = 1.
        return value, settings
    kwargs = {'num_step': steps, 'class_temperature': controls['temperature']}
    if 'duration' in controls:
        settings['duration'] = controls['duration']
        value = checked_audio(tts.synth(text, voice, seed=seed, duration=controls['duration'],
                              postprocess_output=False, pad_duration=0., **kwargs), rate)
        return value, settings
    settings['speed'] = controls['speed']
    kwargs['speed'] = controls['speed']
    if 'sentencePause' not in controls:
        value = checked_audio(tts.synth(text, voice, seed=seed, **kwargs), rate)
        return value, settings
    settings['sentencePause'] = controls['sentencePause']
    pause_frames = round(rate * controls['sentencePause'])
    parts, frames = [], 0
    for index, sentence in enumerate(split_sentences(text)):
        if parts:
            if frames + pause_frames >= rate * MAX_SECONDS:
                raise ValueError('Generated audio is too long')
            parts.append(np.zeros(pause_frames, np.float32))
            frames += pause_frames
        value = checked_audio(tts.synth(sentence, voice, seed=(seed + index) % (2 ** 32), **kwargs), rate, frames)
        parts.append(value)
        frames += len(value)
    if not parts:
        raise ValueError('No speakable text')
    return np.concatenate(parts), settings
