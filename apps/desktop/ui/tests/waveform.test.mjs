import test from 'node:test';
import assert from 'node:assert/strict';
import { waveformPeaks, normalizeWaveformPeaks, formatAudioTime, boundedSeek } from '../src/waveform.js';
test('waveform bins preserve real signed extrema across channels and tail samples', () => {
  assert.deepEqual(waveformPeaks([new Float32Array([0, -.5, .25, 1, -.75])], 2), [{min:-.5,max:0},{min:-.75,max:1}]);
  assert.deepEqual(waveformPeaks([[.5,0],[-.8,.9]],1), [{min:-.8,max:.9}]);
  assert.deepEqual(waveformPeaks([[0,0]],2), [{min:0,max:0},{min:0,max:0}]);
});
test('waveform handles empty/invalid samples, bounds output, and never invents signal', () => {
  assert.deepEqual(waveformPeaks([]), []); assert.deepEqual(waveformPeaks([[NaN,Infinity]],1), [{min:0,max:0}]);
  assert.deepEqual(waveformPeaks([[2,-2]],1), [{min:-1,max:1}]);
  assert.equal(waveformPeaks([new Float32Array(1000)]).length,240);
  assert.deepEqual(waveformPeaks([[1]],0), []);
});
test('display normalization makes quiet peaks visible while preserving zero, sign, relative heights and original data', () => {
  const channels = [new Float32Array([0,-.0004,.0002,0]),new Float32Array([0,-.0002,.0001,0])];
  const before = channels.map(channel => Array.from(channel));
  const raw = waveformPeaks(channels,4), snapshot = structuredClone(raw);
  const display = normalizeWaveformPeaks(raw);
  assert.deepEqual(display,[{min:0,max:0},{min:-.99,max:0},{min:0,max:.495},{min:0,max:0}]);
  assert.deepEqual(channels.map(channel => Array.from(channel)),before,'PCM is never normalized');
  assert.deepEqual(raw,snapshot,'raw extrema remain available unchanged');
  assert.notEqual(display,raw);
});
test('normalization uses one recording-wide scale rather than amplifying each bin independently', () => {
  const display = normalizeWaveformPeaks([{min:-.001,max:.002},{min:-.0001,max:.0002},{min:0,max:0}]);
  assert.equal(display[0].max,.99);assert.equal(display[0].min,-.495);
  assert.equal(display[1].max,.099);assert.equal(display[1].min,-.0495);
  assert.deepEqual(display[2],{min:0,max:0});
  assert.deepEqual(normalizeWaveformPeaks([{min:0,max:.5}]),[{min:0,max:.99}],'do not shift the zero baseline for one-sided audio');
});
test('display normalization leaves silence flat and handles invalid or extremely tiny peaks without nonfinite output', () => {
  assert.deepEqual(normalizeWaveformPeaks([]),[]);
  assert.deepEqual(normalizeWaveformPeaks([{min:0,max:0},{min:NaN,max:Infinity}]),[{min:0,max:0},{min:0,max:0}]);
  assert.deepEqual(normalizeWaveformPeaks([{min:-Number.MIN_VALUE,max:Number.MIN_VALUE}]),[{min:-.99,max:.99}]);
  for(const peak of normalizeWaveformPeaks([{min:-1,max:1},{min:-.2,max:.4}]))for(const value of Object.values(peak))assert.ok(Number.isFinite(value)&&value>=-.99&&value<=.99);
});
test('audio labels format duration without rounding beyond available audio', () => {
  assert.equal(formatAudioTime(0),'0:00'); assert.equal(formatAudioTime(65.9),'1:05');
  assert.equal(formatAudioTime(3601),'1:00:01'); assert.equal(formatAudioTime(NaN),'0:00');
});
test('seeking clamps keyboard/pointer requests to finite actual duration', () => {
  assert.equal(boundedSeek(-3,10),0); assert.equal(boundedSeek(99,10),10);
  assert.equal(boundedSeek(4.2,10),4.2); assert.equal(boundedSeek(Infinity,10),0);
  assert.equal(boundedSeek(3,NaN),0);
});
