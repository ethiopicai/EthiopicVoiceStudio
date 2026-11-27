export function seedDownloadReview(showIntroduction=false){
 if(window.self!==window.top)return;
 window.downloadRequests=[];window.downloadQuoteFails=false;window.downloadAvailable=true;window.introductionSeen=!showIntroduction;
 const base=window.__TAURI_INTERNALS__.invoke;
 window.__TAURI_INTERNALS__.invoke=async(command,args={})=>{
  if(command==='get_setup_introduction')return !window.introductionSeen;
  if(command==='acknowledge_setup_introduction'){window.introductionSeen=true;return;}
  if(command==='get_asset_download_plan'){
   if(window.downloadQuoteFails)throw Error('Unavailable');
   return {id:`quote-${args.kind}`,kind:args.kind,title:'Selected voice tools',available:window.downloadAvailable,reason:window.downloadAvailable?'':'Compatible runtime has not been released.',downloadBytes:120000000,items:[{name:'amharic-model.bin',purpose:'Amharic speech model',bytes:100000000,url:'https://huggingface.co/ethiopic/models/resolve/pin/model.bin'},{name:'speech-runtime.tar.gz',purpose:'Speech engine runtime',bytes:20000000,url:'https://huggingface.co/ethiopic/models/resolve/pin/runtime.tar.gz'}]};
  }
  if(['start_voice_setup','prepare_voice_generation','prepare_voice_edit','start_noise_reduction_setup'].includes(command))window.downloadRequests.push({command,args});
  return base(command,args);
 };
}
