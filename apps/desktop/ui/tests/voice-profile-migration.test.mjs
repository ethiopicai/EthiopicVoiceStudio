import 'fake-indexeddb/auto';
import test from 'node:test';
import assert from 'node:assert/strict';
import {listProfiles,installPresetPack} from '../src/voice-profiles.js';

test('the preset receipt schema upgrade preserves v1 user profile files, custom names and photos',async()=>{
 const saved={id:'user-existing',name:'My customized voice',kind:'reference-draft',language:'am-ET',reference:new File(['original reference bytes'],'my-recording.wav'),referenceText:'ሰላም',image:'data:image/jpeg;base64,AAAA'};
 await new Promise((resolve,reject)=>{const request=indexedDB.open('ethiopic-voice-profiles',1);request.onupgradeneeded=()=>request.result.createObjectStore('profiles',{keyPath:'id'}).put(saved);request.onsuccess=()=>{request.result.close();resolve();};request.onerror=()=>reject(request.error);});
 const voices=await listProfiles();assert.equal(voices.length,1);assert.equal(voices[0].name,saved.name);assert.equal(voices[0].referenceText,saved.referenceText);assert.equal(voices[0].image,saved.image);assert.equal(await voices[0].reference.text(),'original reference bytes');
 await installPresetPack('migration-check',async()=>[{...saved,id:'new-preset',name:'New preview'}]);
 assert.equal((await listProfiles()).length,2);assert.equal((await listProfiles()).find(voice=>voice.id===saved.id).name,saved.name);
});
