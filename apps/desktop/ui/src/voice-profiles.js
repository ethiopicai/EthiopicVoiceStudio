import {DEFAULT_PROFILE_IMAGES} from './profile-image.js';
import { referenceError } from './voice-input.js';
import {recordingTitleError} from './recording-titles.js';

/** @typedef {{name:string,image:string,referenceText:string,audioName:string,bytes:number}} PreviousPreset */
/** @typedef {{id: string, name: string, kind: 'reference-draft', language: 'am-ET', reference: File,image?:string,referenceText?:string,preset?:{packId:string,noncommercial:true,sourceFilename:string,licenseUrl:string,previous?:PreviousPreset,previousByPack?:Record<string,PreviousPreset>,preservedLegacyReference?:boolean},sourceMedia?:import('./recording-transfer.js').RecordingContext|null}} VoiceProfile */

/** @param {string} name @param {File} reference @param {{referenceText?:string,sourceMedia?:import('./recording-transfer.js').RecordingContext|null}} [details] @returns {VoiceProfile} */
export function createProfile(name, reference, details) {
  const cleanName = name.trim();
  if (!cleanName || cleanName.length > 80) throw new Error('Use a profile name between 1 and 80 characters.');
  const error = referenceError(reference);
  if (error) throw new Error(error);
  return { id: crypto.randomUUID(), name: cleanName, kind: 'reference-draft', language: 'am-ET', reference,...(details??{}) };
}

function openProfiles() {
  return new Promise((resolve, reject) => {
    let rejected=false;
    const request = indexedDB.open('ethiopic-voice-profiles', 2);
    request.onupgradeneeded = () => {
      if(!request.result.objectStoreNames.contains('profiles'))request.result.createObjectStore('profiles', { keyPath: 'id' });
      if(!request.result.objectStoreNames.contains('presetPacks'))request.result.createObjectStore('presetPacks', { keyPath: 'id' });
    };
    request.onsuccess = () => {if(rejected){request.result.close();return;}request.result.onversionchange=()=>request.result.close();resolve(request.result);};
    request.onerror = () => reject(new Error('Voice profiles could not be opened on this computer.'));
    request.onblocked = () => {rejected=true;reject(new Error('Close other studio windows and try again.'));};
  });
}

/** Install bundled voices once, with a receipt in the same transaction. User
 * renames, image changes and deletions survive refreshes and future app starts.
 * @param {string} packId @param {()=>Promise<VoiceProfile[]>} loadProfiles
 * @param {{replacesPackId?:string,replacesPackIds?:string[]}} [migration] */
export async function installPresetPack(packId,loadProfiles,migration={}){
  const predecessors=migration.replacesPackIds??(migration.replacesPackId?[migration.replacesPackId]:[]);
  const db=/** @type {IDBDatabase} */(await openProfiles());
  try{
    const installed=await new Promise((resolve,reject)=>{const tx=db.transaction('presetPacks','readonly'),request=tx.objectStore('presetPacks').get(packId);tx.oncomplete=()=>resolve(!!request.result);tx.onerror=()=>reject(Error('Preview voices could not be checked.'));tx.onabort=()=>reject(Error('Checking preview voices was interrupted. Try Refresh voices again.'));});
    if(installed)return;
    const profiles=await loadProfiles();
    if(!profiles.length||new Set(profiles.map(profile=>profile.id)).size!==profiles.length)throw Error('Preview voice pack is incomplete.');
    await new Promise((resolve,reject)=>{
      const tx=db.transaction(['profiles','presetPacks'],'readwrite'),packs=tx.objectStore('presetPacks'),request=packs.get(packId);
      request.onsuccess=()=>{
        if(request.result)return;
        const previous=predecessors.length?packs.getAll():null;
        const write=()=>{
          const store=tx.objectStore('profiles'),existing=store.getAll();
          existing.onsuccess=()=>{
            /** @type {Map<string,VoiceProfile>} */const savedProfiles=new Map(existing.result.map(profile=>[profile.id,profile]));
            for(const profile of profiles){
              const saved=savedProfiles.get(profile.id),savedPack=saved?.preset?.packId;
              const prior=savedPack?(profile.preset?.previousByPack?.[savedPack]??(savedPack===migration.replacesPackId?profile.preset?.previous:undefined)):undefined;
              if(!saved){if(!previous?.result.some(receipt=>predecessors.includes(receipt.id)))store.add(profile);continue;}
              if(saved.preset?.packId===packId)continue;
              if(!savedPack||!predecessors.includes(savedPack)||!prior||!profile.preset){tx.abort();return;}
              const unchangedAudio=saved.reference.name===prior.audioName&&saved.reference.size===prior.bytes&&saved.referenceText===prior.referenceText;
              const updated={...saved,...profile,name:saved.name===prior.name?profile.name:saved.name,image:saved.image===prior.image?profile.image:saved.image};
              if(!unchangedAudio){updated.reference=saved.reference;updated.referenceText=saved.referenceText;updated.preset={...profile.preset,preservedLegacyReference:true};}
              else updated.sourceMedia=null;
              store.put(updated);
            }
            packs.add({id:packId,installedAt:Date.now(),profileIds:profiles.map(profile=>profile.id),replacesPackIds:predecessors});
          };
        };
        if(previous)previous.onsuccess=write;else write();
      };
      tx.oncomplete=()=>resolve(undefined);tx.onerror=()=>reject(Error('Preview voices could not be saved. Your saved profiles are unchanged.'));tx.onabort=()=>reject(Error('Preview voice setup was interrupted. Try Refresh voices again.'));
    });
  }finally{db.close();}
}

/** @returns {Promise<VoiceProfile[]>} */
export async function listProfiles() {
  const db = /** @type {IDBDatabase} */ (await openProfiles());
  try {
    return await new Promise((resolve, reject) => {
      const tx = db.transaction('profiles', 'readonly');
      const request = tx.objectStore('profiles').getAll();
      tx.oncomplete = () => resolve(request.result);
      tx.onerror = () => reject(new Error('Saved voices could not be read.'));
      tx.onabort = () => reject(new Error('Reading saved voices was interrupted.'));
    });
  } finally { db.close(); }
}

/** @param {VoiceProfile} profile */
export async function saveProfile(profile) {
  // Reactive UI proxies are not structured-cloneable. Snapshot JSON metadata,
  // while keeping the actual File as an independent structured-cloned recording.
  const {reference,...metadata}=profile;
  const stored={...JSON.parse(JSON.stringify(metadata)),reference};
  const db = /** @type {IDBDatabase} */ (await openProfiles());
  try {
    await new Promise((resolve, reject) => {
      const tx = db.transaction('profiles', 'readwrite');
      tx.objectStore('profiles').put(stored);
      tx.oncomplete = () => resolve(undefined);
      tx.onerror = () => reject(new Error('This voice could not be saved. Check available storage and try again.'));
      tx.onabort = () => reject(new Error('Saving was interrupted. The voice has not been saved.'));
    });
  } finally { db.close(); }
}

/** @param {string} id */
export async function deleteProfile(id) {
  return deleteProfiles([id]);
}
/** Rename only existing metadata in one transaction; never recreate a deleted profile.
 * @param {string} id @param {string} name @param {string} expectedName @returns {Promise<VoiceProfile>} */
export async function renameProfile(id,name,expectedName){
  const error=recordingTitleError(name,80);if(error)throw Error(error);
  const db=/** @type {IDBDatabase} */(await openProfiles());
  try{return await new Promise((resolve,reject)=>{
    const tx=db.transaction('profiles','readwrite'),store=tx.objectStore('profiles'),request=store.get(id);
    /** @type {VoiceProfile|undefined} */let updated;let failure=Error('The voice title could not be saved.');
    request.onsuccess=()=>{const saved=request.result;if(!saved||saved.name!==expectedName){failure=Error('This voice changed or was deleted. Refresh history and try again.');tx.abort();return;}updated={...saved,name:name.trim()};store.put(updated);};
    tx.oncomplete=()=>resolve(/** @type {VoiceProfile} */(updated));tx.onerror=()=>reject(failure);tx.onabort=()=>reject(failure);
  });}finally{db.close();}
}
/** Update only the image on the latest saved profile, preserving audio and other metadata.
 * @param {string} id @param {string} image @returns {Promise<VoiceProfile>} */
export async function setProfileImage(id,image){
  if(typeof image!=='string'||image.length>200_000||(!DEFAULT_PROFILE_IMAGES.some(preset=>preset.src===image)&&!/^data:image\/jpeg;base64,[A-Za-z0-9+/]+={0,2}$/.test(image)))throw Error('Choose a JPG, PNG or WebP image again.');
  const db=/** @type {IDBDatabase} */(await openProfiles());
  try{return await new Promise((resolve,reject)=>{
    const tx=db.transaction('profiles','readwrite'),store=tx.objectStore('profiles'),request=store.get(id);
    /** @type {VoiceProfile|undefined} */let updated;let failure=Error('The profile image could not be saved. Check available storage and try again.');
    request.onsuccess=()=>{const saved=request.result;if(!saved){failure=Error('This voice was deleted. Refresh history and try again.');tx.abort();return;}updated={...saved,image};store.put(updated);};
    tx.oncomplete=()=>resolve(/** @type {VoiceProfile} */(updated));tx.onerror=()=>reject(failure);tx.onabort=()=>reject(failure);
  });}finally{db.close();}
}
/** Delete selected profile-owned audio and metadata atomically in one transaction.
 * @param {string[]} ids */
export async function deleteProfiles(ids) {
  if(ids.length>5000||ids.some(id=>typeof id!=='string'||!id||id.length>100)||new Set(ids).size!==ids.length)throw new Error('Refresh saved voices and select each profile once.');
  const db = /** @type {IDBDatabase} */ (await openProfiles());
  try {
    await new Promise((resolve, reject) => {
      const tx = db.transaction('profiles', 'readwrite');
      try{for(const id of ids)tx.objectStore('profiles').delete(id);}catch{tx.abort();reject(new Error('Removing the selected voices was interrupted.'));}
      tx.oncomplete = () => resolve(undefined);
      tx.onerror = () => reject(new Error('The saved voice could not be removed.'));
      tx.onabort = () => reject(new Error('Removing the voice was interrupted.'));
    });
  } finally { db.close(); }
}
