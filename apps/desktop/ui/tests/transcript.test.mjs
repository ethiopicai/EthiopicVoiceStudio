import test from 'node:test';
import assert from 'node:assert/strict';
import { transcriptionFileError, MAX_TRANSCRIPTION_BYTES, correctionError, playbackSpan, isActiveJob, timedWordIdAt } from '../src/transcript.js';
test('Audio import rejects empty, oversized, and unsupported files before reading', () => {
  assert.equal(transcriptionFileError({ name: 'speech.WAV', size: MAX_TRANSCRIPTION_BYTES }), '');
  for (const size of [0, -1, NaN]) assert.ok(transcriptionFileError({ name: 'x.wav', size }));
  assert.match(transcriptionFileError({ name: 'x.wav', size: MAX_TRANSCRIPTION_BYTES + 1 }), /100 MB/);
  for (const name of ['x.MP3','x.flac','x.ogg','x.m4a']) assert.equal(transcriptionFileError({name,size:5}),'');
  for (const name of ['x.wav.exe', 'x']) assert.match(transcriptionFileError({ name, size: 5 }), /WAV, MP3/);
});
test('corrections must be explicit nonempty bounded text', () => {
  assert.ok(correctionError('   ')); assert.ok(correctionError('x'.repeat(201)));
  assert.equal(correctionError('  ሰላም  '), '');
});
test('playback uses original seconds and rejects absent or invalid alignment', () => {
  assert.deepEqual(playbackSpan({ start: 1.25, end: 2.4 }), { start: 1.25, end: 2.4 });
  for (const span of [{ start: -1, end: 1 }, { start: 2, end: 1 }, { start: 1, end: 1 }, { start: NaN, end: 2 }]) assert.equal(playbackSpan(span), null);
});
test('terminal jobs never keep polling active or block new imports', () => {
  for (const state of ['queued', 'running', 'cancelling']) assert.equal(isActiveJob(state), true);
  for (const state of ['completed', 'failed', 'cancelled']) assert.equal(isActiveJob(state), false);
});
test('playback highlight respects word boundaries, gaps, seeking, and final end', () => {
  const words = [{id:4,start:.4,end:.8},{id:8,start:1,end:1.5},{id:12,start:1.5,end:2}];
  assert.equal(timedWordIdAt(words,.39),null);
  assert.equal(timedWordIdAt(words,.4),4);
  assert.equal(timedWordIdAt(words,.79),4);
  assert.equal(timedWordIdAt(words,.8),null);
  assert.equal(timedWordIdAt(words,1.5),12);
  assert.equal(timedWordIdAt(words,2),null);
  assert.equal(timedWordIdAt(words,1.2),8);
  assert.equal(timedWordIdAt(words,.5),4);
});
test('highlight safely handles absent timings and preserves edited source spans', () => {
  for (const seconds of [NaN,Infinity,-1]) assert.equal(timedWordIdAt([{id:0,start:0,end:1}],seconds),null);
  assert.equal(timedWordIdAt([],1),null);
  assert.equal(timedWordIdAt([{id:0,start:-1,end:1}],.5),null);
  assert.equal(timedWordIdAt([{id:0,start:0,end:Infinity}],.5),null);
  assert.equal(timedWordIdAt([{id:0,start:0,end:0}],0),null);
  assert.equal(timedWordIdAt([{id:0,start:NaN,end:1}],.5),null);
  assert.equal(timedWordIdAt([{id:7,start:2,end:3,corrected:'two replacement words'}],2.5),7);
});
