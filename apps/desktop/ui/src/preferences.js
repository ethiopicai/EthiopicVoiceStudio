import {derived,get,writable} from 'svelte/store';

export const PREFERENCES_KEY='ethiopic-studio.preferences.v1';
/** @typedef {{theme:'system'|'light'|'dark',microphoneId:string}} Preferences */
/** @type {Preferences} */
const defaults={theme:'system',microphoneId:''};
function storage(){try{return typeof window!=='undefined'?window.localStorage:null;}catch{return null;}}
/** @param {unknown} value @returns {Preferences} */
export function validatePreferences(value){
  const v=/** @type {any} */(value);
  if(!v||!['system','light','dark'].includes(v.theme)||typeof v.microphoneId!=='string'||v.microphoneId.length>1024||/[\x00-\x1f]/.test(v.microphoneId))throw new Error('Invalid preferences');
  return {theme:v.theme,microphoneId:v.microphoneId};
}
/** @param {Pick<Storage,'getItem'>|null} [source] */
export function readPreferences(source=storage()){
  try{const raw=source?.getItem(PREFERENCES_KEY);return raw?validatePreferences(JSON.parse(raw)):{...defaults};}catch{return {...defaults};}
}
export const preferences=writable(readPreferences());
const systemDark=writable(typeof window!=='undefined'&&!!window.matchMedia?.('(prefers-color-scheme: dark)').matches);
export const effectiveTheme=derived([preferences,systemDark],([$p,$dark])=>$p.theme==='system'?($dark?'dark':'light'):$p.theme);
/** Save before notifying consumers, so a failed write keeps the previous setting. IDs stay local. */
/** @param {Partial<Preferences>} update @param {Pick<Storage,'setItem'>|null} [target] */
export function savePreferences(update,target=storage()){
  const next=validatePreferences({...get(preferences),...update});
  if(!target)throw new Error('Settings storage unavailable');
  target.setItem(PREFERENCES_KEY,JSON.stringify(next));preferences.set(next);return next;
}
export function initializePreferences(){
  const media=window.matchMedia('(prefers-color-scheme: dark)');systemDark.set(media.matches);
  const unsubscribe=effectiveTheme.subscribe(theme=>{document.documentElement.dataset.theme=theme;document.documentElement.style.colorScheme=theme;});
  /** @param {MediaQueryListEvent} e */const changed=e=>systemDark.set(e.matches);
  /** @param {StorageEvent} e */const synced=e=>{if(e.key===PREFERENCES_KEY)preferences.set(readPreferences());};
  media.addEventListener('change',changed);window.addEventListener('storage',synced);
  return()=>{unsubscribe();media.removeEventListener('change',changed);window.removeEventListener('storage',synced);};
}
/** @param {string} deviceId @returns {MediaStreamConstraints} */
export function microphoneConstraints(deviceId){
  return {audio:{channelCount:1,echoCancellation:false,noiseSuppression:false,autoGainControl:false,...(deviceId?{deviceId:{exact:deviceId}}:{})}};
}
