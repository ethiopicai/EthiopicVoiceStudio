/** Generation readiness is independent of speech-editing support.
 * @param {{ready:boolean,consent:boolean,canPrepare?:boolean,reason?:string,defaultVoice?:boolean,requiresReferenceText?:boolean}|null} status
 * @param {{style:string,hasReference:boolean,referenceText:string,text:string,busy?:boolean}} input */
export function generationBlocker(status,{style,hasReference,referenceText,text,busy=false}){
 if(busy)return {code:'busy',message:'Your voice task is running. You can cancel it below.'};
 if(!status)return {code:'checking',message:'Checking speech tools…'};
 if(!status.consent)return {code:'consent',message:'Only use recordings you have the right to use. Studio is licensed for non-commercial use; model licenses are separate.'};
 if(!status.ready)return {code:status.canPrepare?'setup':'unavailable',message:status.reason||'Speech tools are not ready. Check their status in Settings.'};
 if(!hasReference&&(!status.defaultVoice||style!=='preset:default'))return {code:'reference',message:'Choose a reference recording'+(status.defaultVoice?' or the Default voice.':'.')};
 if(status.requiresReferenceText&&hasReference&&!referenceText.trim())return {code:'reference-text',message:'Enter the exact words in your reference recording.'};
 if(!text.trim())return {code:'text',message:'Enter the Amharic script you want to generate.'};
 if(text.length>12000)return {code:'length',message:'Use up to 12,000 characters per recording. Split longer scripts into parts.'};
 return null;
}
