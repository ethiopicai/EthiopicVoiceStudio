import test from 'node:test';
import assert from 'node:assert/strict';
import {cropPcmWav,cropDecodedAudio,trimFrames,croppedRecordingContext} from '../src/audio-trim.js';
function pcm(bits=16,channels=1,frames=32000){const block=bits/8*channels,bytes=new Uint8Array(44+frames*block),v=new DataView(bytes.buffer);for(const [at,text] of [[0,'RIFF'],[8,'WAVE'],[12,'fmt '],[36,'data']])for(let i=0;i<text.length;i++)bytes[at+i]=text.charCodeAt(i);v.setUint32(4,bytes.length-8,true);v.setUint32(16,16,true);v.setUint16(20,1,true);v.setUint16(22,channels,true);v.setUint32(24,16000,true);v.setUint32(28,16000*block,true);v.setUint16(32,block,true);v.setUint16(34,bits,true);v.setUint32(40,frames*block,true);for(let i=44;i<bytes.length;i++)bytes[i]=(i*7)%256;return bytes;}
test('cropping PCM preserves selected sample bytes, rate, channels and precision without changing the source',()=>{
 for(const bits of [8,16,24,32])for(const channels of [1,2,6]){
  const source=pcm(bits,channels),snapshot=new Uint8Array(source),block=bits/8*channels,result=cropPcmWav(source,.25,.75),v=new DataView(result.bytes.buffer);
  assert.deepEqual(result.bytes.subarray(44),source.subarray(44+4000*block,44+12000*block));assert.deepEqual(source,snapshot);
  assert.equal(v.getUint16(34,true),bits);assert.equal(v.getUint16(22,true),channels);assert.equal(v.getUint32(24,true),16000);assert.equal(result.end-result.start,.5);
 }
});
test('frame selection rejects invalid and empty ranges, rounds once and pads odd WAV data',()=>{
 const source=pcm(8,1);const result=cropPcmWav(source,0,1/16000),v=new DataView(result.bytes.buffer);
 assert.equal(v.getUint32(40,true),1);assert.equal(result.bytes.length,46);assert.equal(result.bytes[45],0);
 for(const range of [[NaN,1],[-1,1],[1,1],[1,0],[0,3],[Infinity,1]])assert.throws(()=>cropPcmWav(source,...range));
 assert.throws(()=>trimFrames(0,1,0,100));assert.throws(()=>trimFrames(.1,.100001,16000,32000));
 assert.equal(cropPcmWav(new Uint8Array([1,2,3]),0,1),null);
 const withJunk=new Uint8Array(source.length+10);withJunk.set(source.subarray(0,36));withJunk.set([74,85,78,75,1,0,0,0,99,0],36);withJunk.set(source.subarray(36),46);new DataView(withJunk.buffer).setUint32(4,withJunk.length-8,true);assert.deepEqual(cropPcmWav(withJunk,0,1).bytes.subarray(44),source.subarray(44,16044),'skip padded metadata chunks without including them in PCM');
 const truncated=source.slice(0,100);assert.throws(()=>cropPcmWav(truncated,0,1),/incomplete/);
});
test('decoded audio crops all channels into PCM16 without normalizing quiet samples',()=>{
 const a=new Float32Array(16000).fill(.001),b=new Float32Array(16000).fill(-.002),result=cropDecodedAudio([a,b],16000,.25,.75),v=new DataView(result.bytes.buffer);
 assert.equal(v.getUint16(22,true),2);assert.equal(v.getUint32(40,true),8000*4);assert.equal(v.getInt16(44,true),33);assert.equal(v.getInt16(46,true),-66);
 assert.equal(a[4000],Math.fround(.001));assert.equal(b[4000],Math.fround(-.002));
 assert.throws(()=>cropDecodedAudio([a,b.slice(0,10)],16000,0,.5));
});
test('trimmed transfer metadata has a fresh hash/source, rebased complete words and clipped synthetic regions',async()=>{
 const context={schemaVersion:1,source:{kind:'transcript',id:'original'},audioSha256:'old-hash',text:'one two three',words:[{word:'one',start:.2,end:.8},{word:'two',start:1.2,end:1.8},{word:'three',start:2.2,end:2.8}],timingsCurrent:true,modelVersion:'v0.2',transcript:{text:'old transcript'},editedRegions:[{span_out:[1.5,2.5],seed:42},{span_out:[.1,.5]}],lineage:[]},snapshot=JSON.stringify(context),bytes=pcm(),result=await croppedRecordingContext(context,1,3,bytes);
 assert.equal(result.text,'two three');assert.deepEqual(result.words,[{word:'two',start:1.2-1,end:1.8-1},{word:'three',start:2.2-1,end:2.8-1}]);assert.equal(result.timingsCurrent,true);
 assert.equal(result.source.kind,'file');assert.equal(result.source.id,result.audioSha256);assert.equal(result.audioSha256.length,64);assert.equal(result.transcript,null);assert.deepEqual(result.transcriptHistory,[context.transcript]);assert.deepEqual(result.lineage,[context.source]);assert.deepEqual(result.editedRegions,[{span_out:[.5,1.5],synthetic:true,seed:42}]);assert.equal(JSON.stringify(context),snapshot);
});
test('partial-word crops and stale timings clear reference text instead of attaching the full transcript to a shorter recording',async()=>{
 const context={schemaVersion:1,source:{kind:'file',id:'original'},audioSha256:'old',text:'one two',words:[{word:'one',start:0,end:1},{word:'two',start:1,end:2}],timingsCurrent:true,editedRegions:[],lineage:[]};
 for(const [a,b,metadata] of [[.5,2,context],[0,1.5,context],[0,1,{...context,timingsCurrent:false}],[2,3,context]]){
  const result=await croppedRecordingContext(metadata,a,b,pcm());assert.equal(result.text,'');assert.equal(result.timingsCurrent,false);assert.deepEqual(result.words,[]);
 }
 assert.equal(await croppedRecordingContext(null,0,1,pcm()),null);
});
