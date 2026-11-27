/** Streaming mono resampling with a windowed-sinc low-pass filter and bounded history.
 * Input chunks share a single sample clock; flush emits exactly the rounded duration.
 */
export class DictationResampler {
  /** @param {number} inputRate @param {number} [outputRate] */
  constructor(inputRate, outputRate=16000) {
    if (!Number.isFinite(inputRate)||inputRate<8000||inputRate>192000) throw new Error('Unsupported microphone rate');
    this.inputRate=inputRate;this.outputRate=outputRate;this.buffer=/** @type {number[]} */ ([]);this.offset=0;this.total=0;this.next=0;this.closed=false;this.radius=24;
  }
  /** @param {Float32Array | number[]} samples @param {boolean} [flush] */
  push(samples, flush=false) {
    if(this.closed)throw new Error('Recording already ended');
    for(const value of samples)this.buffer.push(Number.isFinite(value)?Math.max(-1,Math.min(1,value)):0);
    this.total+=samples.length;
    const target=Math.round(this.total*this.outputRate/this.inputRate), values=[];
    const cutoff=Math.min(1,this.outputRate/this.inputRate)*.92;
    while(this.next<target){
      const position=this.next*this.inputRate/this.outputRate;
      if(!flush&&position+this.radius>=this.total)break;
      let sum=0,weight=0;
      for(let index=Math.ceil(position-this.radius);index<=Math.floor(position+this.radius);index++){
        const distance=position-index;
        const x=Math.PI*distance*cutoff;
        const w=(x===0?1:Math.sin(x)/x)*(.5+.5*Math.cos(Math.PI*distance/this.radius));
        const bounded=Math.min(this.total-1,Math.max(0,index));
        sum+=(this.buffer[bounded-this.offset]??0)*w;weight+=w;
      }
      values.push(Math.max(-1,Math.min(1,sum/(weight||1))));this.next++;
    }
    const discard=Math.max(0,Math.min(this.buffer.length,Math.floor(this.next*this.inputRate/this.outputRate)-this.radius-this.offset));
    this.buffer.splice(0,discard);this.offset+=discard;
    if(flush){this.closed=true;this.buffer=/** @type {number[]} */ ([]);}
    return Float32Array.from(values);
  }
}

/** Serial, bounded, coalescing transport. Fail rather than silently dropping speech. */
export class DictationSendQueue {
  /** @param {(samples:number[])=>Promise<void>} send @param {number} [maxSamples] */
  constructor(send,maxSamples=160000){this.send=send;this.maxSamples=maxSamples;this.pending=/** @type {Float32Array[]} */ ([]);this.count=0;this.sending=0;this.running=/** @type {Promise<void>|null} */ (null);this.error=/** @type {Error|null} */ (null);this.closed=false;}
  /** @param {Float32Array} samples */
  add(samples){
    if(this.error)throw this.error;
    if(this.closed)throw new Error('Recording already ended');
    if(this.count+this.sending+samples.length>this.maxSamples)throw new Error('Recognition is falling behind. Stop and use transcription after recording.');
    if(samples.length){this.pending.push(samples);this.count+=samples.length;this.start();}
  }
  start(){if(this.running||!this.count)return;this.running=this.drain().catch(error=>{this.error=error;}).finally(()=>{this.running=/** @type {Promise<void>|null} */ (null);if(this.count&&!this.error)this.start();});}
  async drain(){while(this.count){const size=Math.min(32000,this.count),data=new Float32Array(size);let at=0;while(at<size){const chunk=this.pending[0],take=Math.min(chunk.length,size-at);data.set(chunk.subarray(0,take),at);at+=take;if(take===chunk.length)this.pending.shift();else this.pending[0]=chunk.subarray(take);}this.count-=size;this.sending=data.length;try{await this.send(Array.from(data));}finally{this.sending=0;}}}
  async finish(){this.closed=true;while(this.running)await this.running;if(this.error)throw this.error;}
}

/** @param {Float32Array[]} chunks @param {number} [sampleRate] */
export function dictationWav(chunks,sampleRate=16000){
  if(![16000,48000].includes(sampleRate))throw new Error('Unsupported recording rate');
  const frames=chunks.reduce((sum,chunk)=>sum+chunk.length,0);
  const bytes=new Uint8Array(44+frames*2),view=new DataView(bytes.buffer);
  /** @param {number} offset @param {string} value */
  const text=(offset,value)=>{for(let i=0;i<value.length;i++)view.setUint8(offset+i,value.charCodeAt(i));};
  text(0,'RIFF');view.setUint32(4,36+frames*2,true);text(8,'WAVE');text(12,'fmt ');view.setUint32(16,16,true);view.setUint16(20,1,true);view.setUint16(22,1,true);view.setUint32(24,sampleRate,true);view.setUint32(28,sampleRate*2,true);view.setUint16(32,2,true);view.setUint16(34,16,true);text(36,'data');view.setUint32(40,frames*2,true);
  let at=44;for(const chunk of chunks)for(const value of chunk){const finite=Number.isFinite(value)?Math.max(-1,Math.min(1,value)):0;view.setInt16(at,Math.round(finite<0?finite*32768:finite*32767),true);at+=2;}
  return bytes;
}
