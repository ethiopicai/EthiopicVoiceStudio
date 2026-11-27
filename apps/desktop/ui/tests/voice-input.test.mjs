import test from 'node:test';
import assert from 'node:assert/strict';
import { referenceError, MAX_REFERENCE_BYTES } from '../src/voice-input.js';

test('accepts supported audio extensions, including uppercase', () => {
  for (const name of ['voice.WAV', 'a.mp3', 'a.flac', 'a.ogg', 'a.m4a']) {
    assert.equal(referenceError({ name, size: 1024 }), '');
  }
});
test('rejects empty recordings and files over the limit', () => {
  assert.match(referenceError({ name: 'a.wav', size: 0 }), /empty/);
  assert.match(referenceError({ name: 'a.wav', size: MAX_REFERENCE_BYTES + 1 }), /50 MB/);
  assert.equal(referenceError({ name: 'a.wav', size: MAX_REFERENCE_BYTES }), '');
});
test('rejects unsupported files and misleading double extensions', () => {
  for (const name of ['notes.txt', 'voice.wav.exe', 'voice']) {
    assert.match(referenceError({ name, size: 10 }), /audio file/);
  }
});
