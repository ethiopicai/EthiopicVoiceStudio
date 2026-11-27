import {writable,get} from 'svelte/store';
import {invoke} from '@tauri-apps/api/core';
/** @type {import('svelte/store').Writable<Record<string,string>>} */
export const recordingTitles=writable({});
let mutations=0,refreshSequence=0;
/** @param {string} title @param {number} [maximum] */
export function recordingTitleError(title,maximum=120){return !title.trim()||Array.from(title.trim()).length>maximum||/[\u0000-\u001f\u007f-\u009f]/.test(title)?`Use a title from 1 to ${maximum} characters on one line.`:'';}
/** @param {Record<string,string>} titles @param {string} kind @param {string} id @param {string} fallback */
export function displayRecordingTitle(titles,kind,id,fallback){return titles[`${kind}:${id}`]||fallback;}
export async function refreshRecordingTitles(){
  const version=mutations,request=++refreshSequence;
  try{const titles=await invoke('list_recording_titles');if(version!==mutations||request!==refreshSequence)return false;
    if(!titles||typeof titles!=='object'||Array.isArray(titles))return false;
    const valid=Object.fromEntries(Object.entries(titles).filter(([key,title])=>/^(transcript|recording|cleanup-recording|cleanup-result|voice-project):[a-zA-Z0-9-]+$/.test(key)&&typeof title==='string'&&!recordingTitleError(title)));
    recordingTitles.set(valid);return true;
  }catch{return false;}
}
/** @param {string} kind @param {string} id @param {string} title */
export async function renameRecording(kind,id,title){
  const error=recordingTitleError(title);if(error)throw Error(error);
  const key=`${kind}:${id}`,expectedTitle=get(recordingTitles)[key]??null;
  try{const saved=await invoke('rename_recording',{kind,id,title:title.trim(),expectedTitle});if(typeof saved!=='string'||recordingTitleError(saved))throw Error('The title could not be saved.');mutations++;recordingTitles.update(current=>({...current,[key]:saved}));return saved;}
  catch(error){await refreshRecordingTitles();throw error;}
}
/** @param {{kind:string,id:string}[]} removed */
export function forgetRecordingTitles(removed){mutations++;recordingTitles.update(current=>{const next={...current};for(const item of removed)delete next[`${item.kind}:${item.id}`];return next;});}
