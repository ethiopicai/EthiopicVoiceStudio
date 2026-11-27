import {writable} from 'svelte/store';
import {invoke} from '@tauri-apps/api/core';
/** @typedef {{kind:'transcript'|'recording'|'cleanup-recording'|'cleanup'|'editing'|'synthesis',id:string,original?:boolean}} RecordingReference */
/** @typedef {{schemaVersion:number,source:{kind:string,id:string},audioSha256:string,text:string,words:{word:string,start:number,end:number}[],timingsCurrent:boolean,modelVersion?:string|null,transcript?:unknown,transcriptHistory?:unknown[],transcriptAudioSha256?:string|null,editLog?:unknown,editHistory?:unknown[],cleanup?:unknown,cleanupHistory?:unknown[],editedRegions:unknown[],lineage:{kind:string,id:string}[]}} RecordingContext */
/** @typedef {{name:string,bytes:number[]|Uint8Array,context:RecordingContext}} RecordingPackage */
/** @typedef {RecordingReference|{kind:'path',path:string}|{kind:'file',file:File,context?:RecordingContext|null,text?:string}} RecordingInput */
/** `clone` is a compatibility alias for `tts`, not a separate menu target.
 * @typedef {'cleanup'|'clone'|'infilling'|'transcription'|'dictation'|'tts'} RecordingTarget */
export const RECORDING_MIME='application/x-ethiopic-studio-recording';
export const recordingTargets=[{id:'cleanup',label:'Voice Cleanup'},{id:'infilling',label:'Voice Editing'},{id:'transcription',label:'Transcription'},{id:'dictation',label:'Dictation'},{id:'tts',label:'Text to Speech'}];
/** @type {import('svelte/store').Writable<{id:number,target:RecordingTarget,input:RecordingInput}|null>} */
export const pendingRecordingTransfer=writable(null);
let sequence=0;
/** One in-window file drag; no audio/text is embedded in OS drag payloads. */
/** @type {{token:string,input:RecordingInput}|null} */let currentDrag=null;
/** @param {RecordingTarget} target @param {RecordingInput} input */
export function requestRecordingTransfer(target,input){
  // Retain old internal links/import requests while exposing a single speech workspace.
  if(target==='clone')target='tts';
  if(!recordingTargets.some(item=>item.id===target))throw Error('Choose a voice tool.');
  pendingRecordingTransfer.set({id:++sequence,target,input});
  window.dispatchEvent(new CustomEvent('studio-recording-transfer',{detail:{target}}));
}
/** @param {DragEvent} event @param {RecordingInput} input */
export function writeRecordingDrag(event,input){
  if(!event.dataTransfer)return;
  const token=crypto.randomUUID();currentDrag={token,input};
  const payload=input.kind==='file'?{version:1,token}:{version:1,input};
  event.dataTransfer.setData(RECORDING_MIME,JSON.stringify(payload));event.dataTransfer.effectAllowed='copy';
}
export function finishRecordingDrag(){currentDrag=null;}
/** @param {DataTransfer|null} data */
export function canDropRecording(data){return !!data&&[RECORDING_MIME,'Files','text/plain','text/uri-list'].some(type=>Array.from(data.types).includes(type));}
/** @param {DataTransfer|null} data @returns {RecordingInput} */
export function readRecordingDrop(data){
  if(!data)throw Error('Drop a recording card or audio file.');
  const raw=data.getData(RECORDING_MIME);
  if(raw){
    if(raw.length>1600)throw Error('The dragged recording reference is invalid.');
    let value;try{value=JSON.parse(raw);}catch{throw Error('The dragged recording reference is invalid.');}
    if(value.version!==1)throw Error('This recording drag format is unsupported.');
    if(value.token){if(!currentDrag||currentDrag.token!==value.token)throw Error('Drag this recording again from its source card.');return currentDrag.input;}
    const input=value.input;
    if(!input||!['transcript','recording','cleanup-recording','cleanup','editing','synthesis'].includes(input.kind)||typeof input.id!=='string'||!input.id||input.id.length>100||!/^[a-zA-Z0-9-]+$/.test(input.id)||Object.keys(input).some(key=>!['kind','id','original'].includes(key))||(input.original!==undefined&&typeof input.original!=='boolean'))throw Error('The dragged recording reference is invalid.');
    return input;
  }
  if(data.files?.length)return {kind:'file',file:data.files[0]};
  const path=(data.getData('text/uri-list').split(/\r?\n/).find(line=>line&&!line.startsWith('#'))||data.getData('text/plain')).trim();
  if(!path||path.length>32768||!/(\.(wav|mp3|flac|ogg|m4a)["']?$)/i.test(path))throw Error('Drop a recording card, audio file or local audio path.');
  return {kind:'path',path};
}
/** @param {RecordingInput} input @returns {Promise<RecordingPackage>} */
export async function resolveRecording(input){
  let nativeInput;
  if(input.kind==='file'){
    if(!/\.(wav|mp3|flac|ogg|m4a)$/i.test(input.file.name)||input.file.size>100*1024*1024||!input.file.size)throw Error('Choose a nonempty audio recording up to 100 MB.');
    nativeInput={kind:'file',name:input.file.name,bytes:Array.from(new Uint8Array(await input.file.arrayBuffer())),context:input.context??null,text:input.text??null};
  }else nativeInput=input;
  const response=await invoke('resolve_recording_transfer',{input:nativeInput});
  return decodeRecordingPackage(response);
}
/** Binary native handoff: bounded metadata length, UTF-8 JSON, then raw WAV.
 * Keeping PCM out of JSON avoids a large number-array expansion on modest computers.
 * @param {unknown} response @returns {RecordingPackage} */
export function decodeRecordingPackage(response){
  if(response instanceof ArrayBuffer||ArrayBuffer.isView(response)){
    const data=response instanceof ArrayBuffer?new Uint8Array(response):new Uint8Array(response.buffer,response.byteOffset,response.byteLength);
    if(data.length<48||data.length>108*1024*1024+4100)throw Error('The recording transfer is incomplete or too large.');
    const length=new DataView(data.buffer,data.byteOffset,data.byteLength).getUint32(0,true);
    if(length>8*1024*1024+4096||length<2||4+length+44>data.length)throw Error('The recording transfer is incomplete. Try again.');
    const metadata=JSON.parse(new TextDecoder('utf-8',{fatal:true}).decode(data.subarray(4,4+length)));
    return {...metadata,bytes:data.subarray(4+length)};
  }
  // Plain fixture results are used by browser tests without a native process.
  const value=/** @type {RecordingPackage} */(response);
  if(!value||typeof value.name!=='string'||!Array.isArray(value.bytes)||!value.context)throw Error('This recording could not be transferred.');
  return value;
}
/** @param {RecordingPackage} value */
export function recordingFile(value){return new File([value.bytes instanceof Uint8Array?value.bytes:new Uint8Array(value.bytes)],value.name,{type:'audio/wav'});}
/** @param {RecordingContext|null|undefined} context */
export function recordingRegions(context){
  return (context?.editedRegions??[]).flatMap(value=>{const region=/** @type {any} */(value),span=region?.span_out;return Array.isArray(span)&&span.length===2&&Number.isFinite(span[0])&&Number.isFinite(span[1])&&span[0]>=0&&span[1]>=span[0]?[{start:span[0],end:span[1]}]:[];});
}
/** Dictation and Transcription share history, so navigating between them reuses
 * an existing card; importing from another tool creates an independent copy.
 * @param {RecordingPackage} value @param {RecordingInput} [input] */
export async function importRecordingHistory(value,input){
  if(input?.kind!=='file'&&value.context.source.kind==='transcript'){
    try{const transcript=await invoke('get_transcript',{id:value.context.source.id});return {transcript,imported:null};}catch{/* A saved reference may outlive its original source card. */}
  }
  const imported=await invoke('import_transcription_audio',{name:value.name,bytes:Array.from(value.bytes),sourceMedia:value.context});
  const transcript=value.context.text.trim()||value.context.transcript?await invoke('adopt_recording_transfer',{audioId:imported.id}):null;
  return {imported,transcript};
}

/** Convert compressed imports locally before waveform/trim/profile use. WAV keeps its original bytes.
 * @param {File} file @returns {Promise<File>} */
export async function prepareRecordingFile(file){
  if(/\.wav$/i.test(file.name))return file;
  return recordingFile(await resolveRecording({kind:'file',file}));
}
