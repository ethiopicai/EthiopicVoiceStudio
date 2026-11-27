/** Structured content contract for the future first-party Home endpoint.
 * No HTML, remote scripts, asset URLs, or user identifiers are accepted.
 * @typedef {'tts'|'cleanup'|'transcription'|'dictation'|'infilling'|'projects'|'setup'} HomeTarget
 * @typedef {{id:string,title:string,body:string,action?:{label:string,target:HomeTarget}}} HomeCard
 * @typedef {{schemaVersion:1,sections:{id:string,kind:'personalized'|'tip'|'news',title:string,cards:HomeCard[]}[]}} HomeContent
 */

const targets=new Set(['tts','clone','cleanup','transcription','dictation','infilling','projects','setup']);
const kinds=new Set(['personalized','tip','news']);
/** @type {HomeContent} */
const placeholder={schemaVersion:1,sections:[
  {id:'for-you',kind:'personalized',title:'For you',cards:[{id:'personalized-placeholder',title:'A space that grows with you',body:'Helpful suggestions and featured content will appear here. For now, start with your own recording.',action:{label:'Start a transcript',target:'transcription'}}]},
  {id:'today-tip',kind:'tip',title:'Today’s tip',cards:[{id:'review-words',title:'Listen. Follow. Fine-tune.',body:'Turn on “Highlight words during playback” in a transcript card to follow the audio. Click a word to hear its span before making a correction.',action:{label:'Open transcription',target:'transcription'}}]},
  {id:'whats-new',kind:'news',title:'What’s new',cards:[{id:'transcript-cards',title:'Your recordings, neatly together',body:'Each transcript now has its own collapsible card with a waveform, word timings, and correction tools.',action:{label:'Explore transcripts',target:'transcription'}},{id:'voice-library',title:'Prepare your next voice',body:'Write a script with the default voice, use a saved reference voice, or import a new voice in Text to Speech.',action:{label:'Open voice workspace',target:'tts'}}]}
]};

/** @param {unknown} value @param {number} limit */
function text(value,limit){if(typeof value!=='string'||!value.trim()||value.length>limit)throw new Error('Invalid home content text');return value.trim();}
/** Validate and project untrusted future endpoint data into plain, bounded UI content.
 * @param {unknown} payload @returns {HomeContent} */
export function parseHomeContent(payload){
  if(!payload||typeof payload!=='object')throw new Error('Invalid home content');
  const value=/** @type {any} */(payload);
  if(value.schemaVersion!==1||!Array.isArray(value.sections)||value.sections.length<1||value.sections.length>6)throw new Error('Unsupported home content');
  const sectionIds=new Set(),cardIds=new Set();
  const sections=value.sections.map((/** @type {any} */section)=>{
    if(!section||!kinds.has(section.kind)||!Array.isArray(section.cards)||section.cards.length<1||section.cards.length>6)throw new Error('Invalid home section');
    const id=text(section.id,60);if(!/^[a-zA-Z0-9_-]+$/.test(id))throw new Error('Invalid home section ID');if(sectionIds.has(id))throw new Error('Duplicate home section');sectionIds.add(id);
    const cards=section.cards.map((/** @type {any} */card)=>{
      if(!card)throw new Error('Invalid home card');
      const id=text(card.id,60);if(!/^[a-zA-Z0-9_-]+$/.test(id))throw new Error('Invalid home card ID');if(cardIds.has(id))throw new Error('Duplicate home card');cardIds.add(id);
      /** @type {HomeCard} */const result={id,title:text(card.title,100),body:text(card.body,600)};
      if(card.action!==undefined){if(!card.action||!targets.has(card.action.target))throw new Error('Invalid home action');result.action={label:text(card.action.label,60),target:card.action.target==='clone'?'tts':card.action.target};}
      return result;
    });
    return {id,kind:section.kind,title:text(section.title,80),cards};
  });
  return {schemaVersion:1,sections};
}

/** Replace this provider with a native first-party endpoint/cache adapter later.
 * @returns {Promise<unknown>} */
export async function placeholderHomeProvider(){return structuredClone(placeholder);}

/** @param {()=>Promise<unknown>} [provider] */
export async function loadHomeContent(provider=placeholderHomeProvider){
  try{return {content:parseHomeContent(await provider()),fallback:false};}
  catch{return {content:parseHomeContent(structuredClone(placeholder)),fallback:true};}
}
