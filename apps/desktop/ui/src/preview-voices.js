import {installPresetPack} from './voice-profiles.js';

export const PREVIEW_PACK_ID='atts1hg1-am-preview-v3';
export const PREVIOUS_PREVIEW_PACK_ID='atts1hg1-am-preview-v1';
export const PREVIOUS_PREVIEW_PACK_IDS=[PREVIOUS_PREVIEW_PACK_ID,'atts1hg1-am-preview-v2'];
export const PREVIEW_CATALOG='/reference-voices/preview-v3/catalog.json';

/** @param {typeof fetch} [fetcher] */
export async function ensurePreviewVoices(fetcher=fetch){
  return installPresetPack(PREVIEW_PACK_ID,async()=>{
    const response=await fetcher(PREVIEW_CATALOG);if(!response.ok)throw Error('Preview voice catalog is unavailable.');
    const catalog=await response.json();
    if(catalog.schemaVersion!==3||catalog.packId!==PREVIEW_PACK_ID||!Array.isArray(catalog.replacesPackIds)||catalog.replacesPackIds.length!==PREVIOUS_PREVIEW_PACK_IDS.length||PREVIOUS_PREVIEW_PACK_IDS.some(id=>!catalog.replacesPackIds.includes(id))||catalog.noncommercial!==true||!Array.isArray(catalog.voices)||catalog.voices.length!==17)throw Error('Preview voice catalog is incomplete.');
    /** @type {import('./voice-profiles.js').VoiceProfile[]} */const profiles=[];
    for(const voice of catalog.voices){
      if(!/^preview-[a-z-]+-[a-z]{2}$/.test(voice.id)||!/^\/reference-voices\/preview-v3\/[a-z]+_[12]_am\.wav$/.test(voice.audio)||!/^\/profile-images\/presets\/[a-z]+-v2\.jpg$/.test(voice.image)||!voice.referenceText||voice.referenceText!==catalog.text||!voice.license?.noncommercial||voice.license.url!=='https://coqui.ai/cpml'||!voice.source?.filename||!Number.isSafeInteger(voice.bytes)||voice.bytes<100||voice.bytes>8_000_000||!/^[a-f0-9]{64}$/.test(voice.audioSha256)||!Number.isFinite(voice.longerThanV1Seconds)||voice.longerThanV1Seconds<12||!voice.previousDefaults||PREVIOUS_PREVIEW_PACK_IDS.some(id=>{const prior=voice.previousDefaults[id];return !prior||typeof prior.name!=='string'||typeof prior.image!=='string'||typeof prior.referenceText!=='string'||!Number.isSafeInteger(prior.bytes)||typeof prior.audioName!=='string';}))throw Error('Preview voice metadata is invalid.');
      const audio=await fetcher(voice.audio);if(!audio.ok)throw Error('A preview voice could not be opened.');
      const bytes=await audio.arrayBuffer();
      const hash=Array.from(new Uint8Array(await crypto.subtle.digest('SHA-256',bytes)),byte=>byte.toString(16).padStart(2,'0')).join('');
      if(bytes.byteLength!==voice.bytes||hash!==voice.audioSha256)throw Error('A preview voice failed its integrity check.');
      profiles.push({id:voice.id,name:voice.name,kind:'reference-draft',language:'am-ET',image:voice.image,referenceText:voice.referenceText,
        reference:new File([bytes],voice.audio.split('/').at(-1),{type:'audio/wav',lastModified:0}),
        preset:{packId:PREVIEW_PACK_ID,noncommercial:true,sourceFilename:voice.source.filename,licenseUrl:voice.license.url,previousByPack:voice.previousDefaults}});
    }
    return profiles;
  },{replacesPackIds:PREVIOUS_PREVIEW_PACK_IDS});
}
