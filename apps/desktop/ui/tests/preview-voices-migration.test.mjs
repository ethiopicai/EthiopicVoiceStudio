import 'fake-indexeddb/auto';
import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {ensurePreviewVoices,PREVIEW_PACK_ID} from '../src/preview-voices.js';
import {installPresetPack,listProfiles,saveProfile,deleteProfile,createProfile} from '../src/voice-profiles.js';

const publicRoot=new URL('../public/',import.meta.url);
const fileFetch=async path=>new Response(await readFile(new URL(path.slice(1),publicRoot)));
// Node's structuredClone currently turns File into Blob; real browser IDB keeps
// the filename. Preserve that browser behavior for these migration fixtures.
const clone=globalThis.structuredClone;
globalThis.structuredClone=(value,options)=>{
 const copied=clone(value,options);
 if(value?.reference instanceof File&&copied?.reference instanceof Blob)copied.reference=new File([copied.reference],value.reference.name,{type:value.reference.type,lastModified:value.reference.lastModified});
 return copied;
};

for(const version of [1,2])test(`v3 upgrades v${version} without duplicates, preserving customizations, references and deletions`,async()=>{
 await new Promise((resolve,reject)=>{const request=indexedDB.deleteDatabase('ethiopic-voice-profiles');request.onsuccess=resolve;request.onerror=()=>reject(request.error);});
 const previousPackId=`atts1hg1-am-preview-v${version}`;
 const legacy=JSON.parse(await readFile(new URL(`../../../../scripts/reference-voices/preview-v${version}-catalog.json`,import.meta.url),'utf8'));
 const current=JSON.parse(await readFile(new URL('reference-voices/preview-v3/catalog.json',publicRoot),'utf8'));
 const oldProfiles=legacy.voices.map(voice=>({id:voice.id,name:voice.name,kind:'reference-draft',language:'am-ET',image:voice.image,referenceText:voice.referenceText,reference:new File([new Uint8Array(voice.bytes)],voice.audio.split('/').at(-1),{type:'audio/wav'}),preset:{packId:previousPackId,noncommercial:true,sourceFilename:voice.source.filename,licenseUrl:voice.license.url}}));
 await installPresetPack(previousPackId,async()=>oldProfiles);
 const renamed={...oldProfiles[0],name:'My chosen voice',image:'data:image/jpeg;base64,AAAA'};await saveProfile(renamed);
 await deleteProfile(oldProfiles[1].id);
 const corrected={...oldProfiles[2],referenceText:'My manually corrected old reference'};await saveProfile(corrected);
 const own=createProfile('My own recording',new File(['own audio'],'mine.wav'));await saveProfile(own);
 await ensurePreviewVoices(fileFetch);
 const profiles=await listProfiles();assert.equal(profiles.length,17);assert.equal(new Set(profiles.map(profile=>profile.id)).size,17);
 const upgraded=profiles.find(profile=>profile.id===oldProfiles[0].id);assert.equal(upgraded.name,renamed.name);assert.equal(upgraded.image,renamed.image);assert.equal(upgraded.referenceText,current.text);assert.equal(upgraded.preset.packId,PREVIEW_PACK_ID);
 assert.equal(upgraded.reference.size,current.voices.find(voice=>voice.id===upgraded.id).bytes);
 assert.ok(!profiles.some(profile=>profile.id===oldProfiles[1].id));
 const preserved=profiles.find(profile=>profile.id===corrected.id);assert.equal(preserved.referenceText,corrected.referenceText);assert.equal(preserved.reference.size,corrected.reference.size);assert.equal(preserved.preset.preservedLegacyReference,true);
 const fresh=profiles.find(profile=>profile.id===oldProfiles[3].id),expected=current.voices.find(voice=>voice.id===fresh.id);assert.equal(fresh.name,expected.name);assert.equal(fresh.image,expected.image);assert.equal(fresh.referenceText,current.text);
 assert.equal(await profiles.find(profile=>profile.id===own.id).reference.text(),'own audio');
 await deleteProfile(fresh.id);await ensurePreviewVoices(()=>{throw Error('receipt must prevent a second install');});assert.ok(!(await listProfiles()).some(profile=>profile.id===fresh.id));
});

test('an unrelated ID collision rolls back every queued preset change and leaves no receipt',async()=>{
 const original=createProfile('Keep this custom voice',new File(['unchanged'],'custom.wav'));await saveProfile(original);
 const proposed=createProfile('New but rolled back',new File(['new'],'new.wav'));
 await assert.rejects(installPresetPack('collision-pack',async()=>[proposed,{...proposed,id:original.id}]),/interrupted|saved/);
 let profiles=await listProfiles();assert.equal(profiles.find(profile=>profile.id===original.id).name,original.name);assert.ok(!profiles.some(profile=>profile.id===proposed.id));
 await installPresetPack('collision-pack',async()=>[proposed]);profiles=await listProfiles();assert.ok(profiles.some(profile=>profile.id===proposed.id));
});
