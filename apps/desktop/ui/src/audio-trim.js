/** Sample-aligned, non-destructive reference trimming. No amplitude normalization. */
const LIMIT=50*1024*1024;
/** @param {number} start @param {number} end @param {number} rate @param {number} length */
export function trimFrames(start,end,rate,length){
  if(!Number.isFinite(start)||!Number.isFinite(end)||!Number.isInteger(rate)||rate<8000||rate>192000||!Number.isInteger(length)||length<1||start<0||end<=start||start>=length/rate||end>length/rate+.001)throw Error('Choose a start and end inside the recording.');
  const from=Math.max(0,Math.min(length,Math.round(start*rate))),to=Math.max(0,Math.min(length,Math.round(end*rate)));
  if(to<=from)throw Error('Select a longer part of the recording.');
  return {from,to,start:from/rate,end:to/rate};
}
/** @param {number} rate @param {number} channels @param {number} bits @param {Uint8Array} data */
function wav(rate,channels,bits,data){
  if(data.length+44+(data.length%2)>LIMIT)throw Error('The trimmed WAV must be smaller than 50 MB. Choose a shorter segment.');
  const bytes=new Uint8Array(44+data.length+(data.length%2)),v=new DataView(bytes.buffer);
  const tags=/** @type {[number,string][]} */([[0,'RIFF'],[8,'WAVE'],[12,'fmt '],[36,'data']]);
  for(const [at,text] of tags)for(let i=0;i<text.length;i++)bytes[at+i]=text.charCodeAt(i);
  v.setUint32(4,bytes.length-8,true);v.setUint32(16,16,true);v.setUint16(20,1,true);v.setUint16(22,channels,true);v.setUint32(24,rate,true);v.setUint32(28,rate*channels*bits/8,true);v.setUint16(32,channels*bits/8,true);v.setUint16(34,bits,true);v.setUint32(40,data.length,true);bytes.set(data,44);return bytes;
}
/** Copy integer PCM frames byte-for-byte. Other codecs use the shared player's decoder.
 * @param {Uint8Array} bytes @param {number} start @param {number} end */
export function cropPcmWav(bytes,start,end){
  const view=new DataView(bytes.buffer,bytes.byteOffset,bytes.byteLength);
  const tag=/** @param {number} at */at=>String.fromCharCode(...bytes.subarray(at,at+4));
  if(bytes.length<12||tag(0)!=='RIFF'||tag(8)!=='WAVE')return null;
  const limit=view.getUint32(4,true)+8;if(limit>bytes.length||limit<12)throw Error('This WAV is incomplete. Import the original recording again.');
  let format=null,data=null;
  for(let at=12;at+8<=limit;){const size=view.getUint32(at+4,true),from=at+8,to=from+size;if(to>limit)throw Error('This WAV is incomplete.');
    if(tag(at)==='fmt '){if(format||size<16)throw Error('The WAV format is invalid.');format={code:view.getUint16(from,true),channels:view.getUint16(from+2,true),rate:view.getUint32(from+4,true),block:view.getUint16(from+12,true),bits:view.getUint16(from+14,true)};}
    if(tag(at)==='data'){if(data)throw Error('This WAV has multiple audio streams. Export a standard WAV first.');data=bytes.subarray(from,to);}at=to+(size%2);
  }
  if(!format||!data)throw Error('This WAV has no readable audio.');
  if(format.code!==1)return null;
  const {channels,rate,block,bits}=format;
  if(channels<1||channels>8||![8,16,24,32].includes(bits)||block!==channels*bits/8||data.length%block)throw Error('Use a standard mono or multichannel PCM WAV.');
  const range=trimFrames(start,end,rate,data.length/block);
  return {...range,bytes:wav(rate,channels,bits,data.subarray(range.from*block,range.to*block)),sampleRate:rate};
}
/** Compressed/reference audio decoded by Web Audio is exported once as PCM16 WAV.
 * @param {Float32Array[]} channels @param {number} rate @param {number} start @param {number} end */
export function cropDecodedAudio(channels,rate,start,end){
  if(!channels.length||channels.length>8||channels.some(c=>c.length!==channels[0].length))throw Error('This recording has unsupported audio channels.');
  const range=trimFrames(start,end,rate,channels[0].length),length=(range.to-range.from)*channels.length*2;
  if(length+44>LIMIT)throw Error('The trimmed WAV must be smaller than 50 MB. Choose a shorter segment.');
  const data=new Uint8Array(length),view=new DataView(data.buffer);let offset=0;
  for(let i=range.from;i<range.to;i++)for(const channel of channels){const value=Number.isFinite(channel[i])?Math.max(-1,Math.min(1,channel[i])):0;view.setInt16(offset,Math.round(value*(value<0?32768:32767)),true);offset+=2;}
  return {...range,bytes:wav(rate,channels.length,16,data),sampleRate:rate};
}
/** Rebase current word/edited-region timings; never reuse a full-length transcript for cropped audio.
 * @param {import('./recording-transfer.js').RecordingContext|null} context @param {number} start @param {number} end @param {Uint8Array} bytes
 * @returns {Promise<import('./recording-transfer.js').RecordingContext|null>} */
export async function croppedRecordingContext(context,start,end,bytes){
  if(!context)return null;
  const hash=Array.from(new Uint8Array(await crypto.subtle.digest('SHA-256',new Uint8Array(bytes)))).map(b=>b.toString(16).padStart(2,'0')).join('');
  const original=JSON.parse(JSON.stringify(context)),overlap=context.words.filter(w=>w.end>start&&w.start<end);
  const complete=context.timingsCurrent&&overlap.length>0&&overlap.every(w=>w.start>=start-.000001&&w.end<=end+.000001);
  const words=complete?overlap.map(w=>({word:w.word,start:Math.max(0,w.start-start),end:Math.min(end-start,w.end-start)})):[];
  const editedRegions=context.editedRegions.flatMap(value=>{const r=/** @type {any} */(value),span=r?.span_out;if(!Array.isArray(span)||span.length!==2||!span.every(Number.isFinite))return [];const a=Math.max(start,span[0]),b=Math.min(end,span[1]);return b>a?[{span_out:[a-start,b-start],synthetic:true,...(r.model?{model:r.model}:{}),...(r.seed!=null?{seed:r.seed}:{})}]:[];});
  return {schemaVersion:1,source:{kind:'file',id:hash},audioSha256:hash,text:words.map(w=>w.word).join(' '),words,timingsCurrent:complete,modelVersion:context.modelVersion??null,transcript:null,transcriptAudioSha256:null,transcriptHistory:[...(original.transcriptHistory??[]),...(original.transcript?[original.transcript]:[])].slice(-32),editLog:null,editHistory:[...(original.editHistory??[]),...(original.editLog?[original.editLog]:[])].slice(-32),cleanup:null,cleanupHistory:[...(original.cleanupHistory??[]),...(original.cleanup?[original.cleanup]:[])].slice(-32),editedRegions,lineage:[...original.lineage,original.source].slice(-32)};
}
