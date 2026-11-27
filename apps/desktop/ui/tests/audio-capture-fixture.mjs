// Observe real fake-device worklet messages without changing the app's audio.
// Tests wait on samples rather than assuming a CI runner produces audio on time.
export function observeCaptureSamples(){
 if(window.self!==window.top)return;
 window.capturedSamples=0;
 const NativeNode=window.AudioWorkletNode;
 window.AudioWorkletNode=class extends NativeNode{
  constructor(...args){
   super(...args);window.capturedSamples=0;
   this.port.addEventListener('message',event=>{window.capturedSamples+=event.data.samples?.length??0;});
   this.port.start();
  }
 };
}
