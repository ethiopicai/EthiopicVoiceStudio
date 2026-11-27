// Local microphone capture only. Recognition runs in the managed native worker.
class DictationCapture extends AudioWorkletProcessor {
  constructor(){super();this.buffer=new Float32Array(4096);this.at=0;this.stopped=false;this.port.onmessage=event=>{if(event.data==='stop'){this.flush();this.stopped=true;this.port.postMessage({stopped:true});}};}
  flush(){if(this.at){const samples=this.buffer.slice(0,this.at);this.port.postMessage({samples},[samples.buffer]);this.at=0;}}
  process(inputs){if(this.stopped)return false;const channels=inputs[0];if(channels?.length){for(let i=0;i<channels[0].length;i++){let value=0;for(const channel of channels)value+=channel[i]||0;this.buffer[this.at++]=value/channels.length;if(this.at===this.buffer.length)this.flush();}}return true;}
}
registerProcessor('dictation-capture',DictationCapture);
