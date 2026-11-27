import test from 'node:test';
import assert from 'node:assert/strict';
import {RECORDING_MIME,writeRecordingDrag,finishRecordingDrag,readRecordingDrop,recordingRegions,resolveRecording,decodeRecordingPackage,prepareRecordingFile,recordingTargets,requestRecordingTransfer,pendingRecordingTransfer} from '../src/recording-transfer.js';
import {get} from 'svelte/store';
test('speech appears once in transfer menus and legacy clone requests keep their recording',()=>{
  const originalWindow=globalThis.window,events=[];globalThis.window={dispatchEvent:event=>events.push(event.detail)};
  try{
    assert.equal(recordingTargets.filter(target=>target.id==='tts').length,1);assert.equal(recordingTargets.some(target=>target.id==='clone'),false);
    const input={kind:'transcript',id:'saved-voice'};requestRecordingTransfer('clone',input);
    assert.equal(get(pendingRecordingTransfer).target,'tts');assert.equal(get(pendingRecordingTransfer).input,input);assert.deepEqual(events,[{target:'tts'}]);
  }finally{pendingRecordingTransfer.set(null);if(originalWindow===undefined)delete globalThis.window;else globalThis.window=originalWindow;}
});
function data(){const values=new Map();return {files:[],types:[],setData(type,value){values.set(type,value);this.types.push(type);},getData:type=>values.get(type)||''};}
test('card drag carries only a versioned reference, with native validation left authoritative',()=>{
  const transfer=data();writeRecordingDrag({dataTransfer:transfer},{kind:'transcript',id:'saved-123'});
  assert.deepEqual(readRecordingDrop(transfer),{kind:'transcript',id:'saved-123'});
  assert.equal(transfer.effectAllowed,'copy');assert.equal(transfer.getData(RECORDING_MIME).includes('bytes'),false);
  transfer.setData(RECORDING_MIME,JSON.stringify({version:1,input:{kind:'transcript',id:'../secret'}}));assert.throws(()=>readRecordingDrop(transfer),/invalid/);
  transfer.setData(RECORDING_MIME,JSON.stringify({version:2,input:{kind:'transcript',id:'saved-123'}}));assert.throws(()=>readRecordingDrop(transfer),/unsupported/);
});
test('file drags keep audio and transcript data inside the current window and expire after dragging',()=>{
  const transfer=data(),file=new File(['fixture'],'reference.wav');const input={kind:'file',file,text:'private reference text'};
  writeRecordingDrag({dataTransfer:transfer},input);
  const raw=transfer.getData(RECORDING_MIME);assert.equal(raw.includes('private reference'),false);assert.equal(raw.includes('fixture'),false);
  assert.equal(readRecordingDrop(transfer),input);finishRecordingDrag();assert.throws(()=>readRecordingDrop(transfer),/Drag this recording again/);
});
test('file and path drops remain explicit and invalid text does not become audio',()=>{
  const transfer=data();transfer.setData('text/plain','/Local folder/recording.wav');assert.deepEqual(readRecordingDrop(transfer),{kind:'path',path:'/Local folder/recording.wav'});
  transfer.setData('text/plain','/Local folder/reference.mp3');assert.deepEqual(readRecordingDrop(transfer),{kind:'path',path:'/Local folder/reference.mp3'});
  transfer.setData('text/plain','ordinary transcript text');assert.throws(()=>readRecordingDrop(transfer),/audio/);
  transfer.files=[new File(['data'],'external.wav')];assert.equal(readRecordingDrop(transfer).file.name,'external.wav');
});
test('edited region overlays use valid numeric spans only',()=>{
  assert.deepEqual(recordingRegions({editedRegions:[{span_out:[.1,.4]},{span_out:['bad',2]},{span_out:[2,1]},{span_out:[0,Infinity]},null]}),[{start:.1,end:.4}]);
});
test('invalid and oversized files are rejected before reading or sending audio',async()=>{
  await assert.rejects(resolveRecording({kind:'file',file:new File([],'empty.wav')}),/nonempty audio/);
  await assert.rejects(resolveRecording({kind:'file',file:{name:'large.wav',size:101*1024*1024,arrayBuffer:()=>{throw Error('must not read');}}}),/100 MB/);
});
test('native binary frames preserve UTF-8 metadata and expose PCM as a compact byte view',()=>{
  const metadata={name:'ድምጽ.wav',context:{text:'ጤና',words:[{word:'ጤና',start:.1,end:.3}]}};
  const json=new TextEncoder().encode(JSON.stringify(metadata)),wav=new Uint8Array(64).fill(7),packet=new Uint8Array(4+json.length+wav.length);
  new DataView(packet.buffer).setUint32(0,json.length,true);packet.set(json,4);packet.set(wav,4+json.length);
  const result=decodeRecordingPackage(packet.buffer);assert.equal(result.name,metadata.name);assert.equal(result.context.text,metadata.context.text);assert.ok(result.bytes instanceof Uint8Array);assert.deepEqual(result.bytes,wav);assert.equal(result.bytes.buffer,packet.buffer);
  new DataView(packet.buffer).setUint32(0,packet.length,true);assert.throws(()=>decodeRecordingPackage(packet),/incomplete/);
});

test('compressed references use native decoding before waveform/trim and WAV stays byte-identical',async()=>{
  let calls=0;const wav=new File(['unchanged'],'reference.wav');assert.equal(await prepareRecordingFile(wav),wav);
  globalThis.window={__TAURI_INTERNALS__:{invoke:async(command,{input})=>{calls++;assert.equal(command,'resolve_recording_transfer');assert.equal(input.name,'reference.MP3');return {name:'reference.wav',bytes:[82,73,70,70],context:{}};}}};
  try{const converted=await prepareRecordingFile(new File(['compressed'],'reference.MP3'));assert.equal(converted.name,'reference.wav');assert.equal(converted.type,'audio/wav');assert.deepEqual([...new Uint8Array(await converted.arrayBuffer())],[82,73,70,70]);assert.equal(calls,1);}
  finally{delete globalThis.window;}
});
