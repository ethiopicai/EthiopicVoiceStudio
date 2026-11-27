import {get,writable} from 'svelte/store';
import {invoke} from '@tauri-apps/api/core';

export type UpdateStatus={currentVersion:string;automaticChecks:boolean;usageStatistics:boolean;lastCheckedAt:number|null;lastSuccessfulCheckAt:number|null;checking:boolean;availableUpdate:{version:string;notes:string;url:string}|null;showNotification:boolean;error:string|null};
export const appUpdates=writable<UpdateStatus|null>(null);
export const updateActionError=writable('');
let checking=false;
function desktop(){return typeof window!=='undefined'&&'__TAURI_INTERNALS__' in window;}
function accept(value:UpdateStatus){if(value&&typeof value.currentVersion==='string')appUpdates.set(value);}
export async function refreshUpdateStatus(){if(!desktop())return;try{accept(await invoke<UpdateStatus>('get_app_update_status'));}catch{updateActionError.set('Update settings could not be read. Please try again.');}}
export async function checkAppUpdate(manual=false){
  if(!desktop()||checking)return;
  if(!manual&&(!navigator.onLine||get(appUpdates)?.automaticChecks===false))return;
  checking=true;updateActionError.set('');appUpdates.update(value=>value?{...value,checking:true}:value);
  try{accept(await invoke<UpdateStatus>('check_app_update',{manual}));}
  catch{if(manual)updateActionError.set('Could not check for updates. Please try again.');}
  finally{checking=false;appUpdates.update(value=>value?{...value,checking:false}:value);}
}
export async function saveUpdatePreferences(automaticChecks:boolean,usageStatistics:boolean){
  updateActionError.set('');
  try{accept(await invoke<UpdateStatus>('set_app_update_preferences',{automaticChecks,usageStatistics}));}
  catch{updateActionError.set('Your update preferences could not be saved. Please try again.');}
}
export async function dismissAppUpdate(){try{accept(await invoke<UpdateStatus>('dismiss_app_update'));}catch{updateActionError.set('This update notice could not be dismissed. Please try again.');}}
export async function openAppUpdate(){updateActionError.set('');try{await invoke('open_app_update');}catch{updateActionError.set('The update page could not be opened. Please try again.');}}
export function initializeAppUpdates(){
  if(!desktop())return()=>{};
  let active=true;
  void refreshUpdateStatus().then(()=>{if(active)void checkAppUpdate();});
  const online=()=>{if(active)void checkAppUpdate();};
  window.addEventListener('online',online);
  window.addEventListener('focus',online);
  const visible=()=>{if(document.visibilityState==='visible')online();};
  document.addEventListener('visibilitychange',visible);
  // Native code enforces a daily interval, including failed automatic attempts.
  const timer=window.setInterval(online,24*60*60*1000);
  return()=>{active=false;window.clearInterval(timer);window.removeEventListener('online',online);window.removeEventListener('focus',online);document.removeEventListener('visibilitychange',visible);};
}
