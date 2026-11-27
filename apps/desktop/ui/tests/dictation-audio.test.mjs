import test from 'node:test';
import assert from 'node:assert/strict';
import {DictationResampler,DictationSendQueue,dictationWav} from '../src/dictation-audio.js';
for(const rate of [44100,48000,16000])test(`resampling ${rate} Hz has continuous chunk boundaries and exact duration`,()=>{
 const input=Float32Array.from({length:rate},(_,i)=>Math.sin(2*Math.PI*440*i/rate)*.5);
 const whole=new DictationResampler(rate).push(input,true);
 const resampler=new DictationResampler(rate),chunks=[];for(let i=0;i<input.length;i+=317)chunks.push(...resampler.push(input.subarray(i,i+317)));chunks.push(...resampler.push([],true));
 assert.equal(chunks.length,16000);assert.deepEqual(Float32Array.from(chunks),whole);let error=0;for(let i=100;i<15900;i++)error+=Math.abs(whole[i]-.5*Math.sin(2*Math.PI*440*i/16000));assert.ok(error/15800<.003);
});
test('resampling sanitizes nonfinite values and attenuates frequencies above output Nyquist',()=>{
 const clean=new DictationResampler(48000).push(Float32Array.from([NaN,Infinity,-Infinity,4,-4,...Array(100).fill(.5)]),true);assert.ok(clean.every(v=>Number.isFinite(v)&&Math.abs(v)<=1));
 const high=new DictationResampler(48000).push(Float32Array.from({length:48000},(_,i)=>Math.sin(2*Math.PI*12000*i/48000)),true);const rms=Math.sqrt(high.slice(100,-100).reduce((s,v)=>s+v*v,0)/(high.length-200));assert.ok(rms<.03);
});
test('PCM WAV preserves duration, has valid header and clips safely',()=>{const bytes=dictationWav([Float32Array.from([-2,-1,0,1,2,NaN])]),v=new DataView(bytes.buffer);assert.equal(new TextDecoder().decode(bytes.slice(0,4)),'RIFF');assert.equal(v.getUint32(24,true),16000);assert.equal(v.getUint32(40,true),12);assert.deepEqual(Array.from({length:6},(_,i)=>v.getInt16(44+i*2,true)),[-32768,-32768,0,32767,32767,0]);});
test('noise-reduction capture keeps its original 48 kHz rate and sample clock in saved WAV',()=>{
 const source=Float32Array.from({length:44100},(_,i)=>Math.sin(2*Math.PI*440*i/44100)*.1),resampler=new DictationResampler(44100,48000);
 const samples=resampler.push(source,true);assert.equal(samples.length,48000);
 const bytes=dictationWav([samples],48000),view=new DataView(bytes.buffer);assert.equal(view.getUint32(24,true),48000);assert.equal(view.getUint32(28,true),96000);assert.equal(view.getUint32(40,true)/2/48000,1);assert.throws(()=>dictationWav([],44100));
});
test('transport serializes requests and coalesces backlog without losing samples',async()=>{let release;const first=new Promise(resolve=>release=resolve),sent=[];let concurrent=0;const queue=new DictationSendQueue(async samples=>{assert.equal(concurrent++,0);sent.push(samples);if(sent.length===1)await first;concurrent--;});queue.add(Float32Array.of(1));queue.add(Float32Array.of(2));queue.add(Float32Array.of(3,4));release();await queue.finish();assert.deepEqual(sent,[[1],[2,3,4]]);});
test('transport bounds queued plus inflight audio and propagates feed failures',async()=>{let release;const queue=new DictationSendQueue(()=>new Promise(r=>release=r),3);queue.add(Float32Array.of(1,2));assert.throws(()=>queue.add(Float32Array.of(3,4)),/falling behind/);release();await queue.finish();const broken=new DictationSendQueue(async()=>{throw new Error('worker stopped');});broken.add(Float32Array.of(1));await assert.rejects(broken.finish(),/worker stopped/);});
test('transport splits a large coalesced tail to fit the native 32000-sample request limit',async()=>{let release;const ready=new Promise(resolve=>release=resolve),sent=[];const queue=new DictationSendQueue(async samples=>{sent.push(samples);if(sent.length===1)await ready;});queue.add(Float32Array.of(.1));queue.add(new Float32Array(70000).fill(.25));release();await queue.finish();assert.equal(sent.reduce((sum,chunk)=>sum+chunk.length,0),70001);assert.ok(sent.every(chunk=>chunk.length<=32000));assert.equal(sent.flat().at(-1),.25);});
test('capture worklet mixes channels, transfers the tail before stop acknowledgement, and stops capturing',async()=>{
 const {readFile}=await import('node:fs/promises');const {runInNewContext}=await import('node:vm');let Capture;const messages=[];
 runInNewContext(await readFile(new URL('../public/dictation-capture.js',import.meta.url),'utf8'),{AudioWorkletProcessor:class{constructor(){this.port={postMessage:value=>messages.push(value),onmessage:null};}},registerProcessor:(_name,implementation)=>Capture=implementation,Float32Array});
 const capture=new Capture();assert.equal(capture.process([[Float32Array.of(.5,.25),Float32Array.of(-.5,.75)]]),true);assert.equal(messages.length,0);capture.port.onmessage({data:'stop'});assert.deepEqual(Array.from(messages[0].samples),[0,.5]);assert.equal(messages[1].stopped,true);assert.equal(capture.process([[Float32Array.of(1)]]),false);assert.equal(messages.length,2);
});
