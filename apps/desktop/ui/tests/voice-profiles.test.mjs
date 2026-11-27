import 'fake-indexeddb/auto';
import test from 'node:test';
import assert from 'node:assert/strict';
import { createProfile, saveProfile, listProfiles, deleteProfile, deleteProfiles, renameProfile, setProfileImage } from '../src/voice-profiles.js';

test('profile creation validates names and reference recordings', () => {
  const file = new File(['audio fixture'], 'reference.wav');
  const profile = createProfile('  My voice  ', file);
  assert.equal(profile.name, 'My voice');
  assert.equal(profile.kind, 'reference-draft');
  assert.equal(profile.language, 'am-ET');
  assert.throws(() => createProfile(' ', file), /name/);
  assert.throws(() => createProfile('a'.repeat(81), file), /name/);
  assert.throws(() => createProfile('Name', new File([], 'empty.wav')), /empty/);
});

test('profiles retain reference data across reopened databases and can be deleted', async () => {
  const first = createProfile('First', new File(['reference one'], 'one.wav'));
  const second = createProfile('Second', new File(['reference two'], 'two.wav'));
  await saveProfile(first);
  await saveProfile(second);
  const restored = await listProfiles();
  assert.equal(restored.length, 2);
  assert.equal(await restored.find(profile => profile.id === first.id).reference.text(), 'reference one');
  assert.equal(restored.find(profile => profile.id === first.id).name, 'First');
  await deleteProfile(first.id);
  assert.deepEqual((await listProfiles()).map(profile => profile.id), [second.id]);
  await deleteProfile(second.id);
  assert.deepEqual(await listProfiles(), []);
});

test('duplicate display names retain separate profile identities', () => {
  const file = new File(['a'], 'a.wav');
  assert.notEqual(createProfile('Same', file).id, createProfile('Same', file).id);
});
test('reference profiles keep transcript and source metadata with their independent audio copy',async()=>{
  const source={schemaVersion:1,source:{kind:'transcript',id:'original-card'},audioSha256:'fixture',text:'ጤና',words:[{word:'ጤና',start:.1,end:.3}],timingsCurrent:true,modelVersion:'v0.2',editedRegions:[],lineage:[]};
  const profile=createProfile('Transferred voice',new File(['independent audio'],'reference.wav'),{referenceText:'ጤና',sourceMedia:new Proxy(source,{})});
  await saveProfile(profile);source.text='changed elsewhere';
  const restored=(await listProfiles()).find(item=>item.id===profile.id);
  assert.equal(restored.referenceText,'ጤና');assert.equal(restored.sourceMedia.text,'ጤና');assert.equal(restored.sourceMedia.modelVersion,'v0.2');assert.equal(restored.sourceMedia.words[0].end,.3);assert.equal(await restored.reference.text(),'independent audio');await deleteProfile(profile.id);
});
test('batch profile deletion validates the selection and leaves unselected files and profiles unchanged',async()=>{
  const original=new File(['original bytes'],'original.wav');
  const first=createProfile('First batch profile',original),second=createProfile('Keep batch profile',original);
  await saveProfile(first);await saveProfile(second);
  await assert.rejects(deleteProfiles([first.id,first.id]));
  assert.equal((await listProfiles()).length,2);
  await deleteProfiles([first.id,'already-removed']);
  assert.deepEqual((await listProfiles()).map(item=>item.id),[second.id]);
  assert.equal(await original.text(),'original bytes');
  await deleteProfiles([second.id]);
});
test('a synchronous batch deletion failure rolls back previously queued profile deletions',async()=>{
  const first=createProfile('Rollback one',new File(['one'],'one.wav')),second=createProfile('Rollback two',new File(['two'],'two.wav'));
  await saveProfile(first);await saveProfile(second);
  const remove=IDBObjectStore.prototype.delete;
  try{IDBObjectStore.prototype.delete=function(id){if(id===second.id)throw new Error('Storage failure');return remove.call(this,id);};await assert.rejects(deleteProfiles([first.id,second.id]));}
  finally{IDBObjectStore.prototype.delete=remove;}
  assert.deepEqual(new Set((await listProfiles()).map(item=>item.id)),new Set([first.id,second.id]));
  await deleteProfiles([first.id,second.id]);
});
test('profile rename persists metadata without changing its reference and cannot recreate deleted profiles',async()=>{
 const profile=createProfile('Original title',new File(['same audio'],'original.wav'),{referenceText:'ጤና'});await saveProfile(profile);
 const renamed=await renameProfile(profile.id,'የፖድካስት ድምጽ','Original title');assert.equal(renamed.id,profile.id);assert.equal(profile.reference.name,'original.wav');assert.equal(await renamed.reference.text(),'same audio');assert.equal(renamed.referenceText,'ጤና');
 assert.equal((await listProfiles()).find(item=>item.id===profile.id).name,'የፖድካስት ድምጽ');await assert.rejects(renameProfile(profile.id,'Stale','Original title'));
 await deleteProfile(profile.id);await assert.rejects(renameProfile(profile.id,'Restore deleted','የፖድካስት ድምጽ'));assert.equal((await listProfiles()).length,0);
});

test('profile images persist without overwriting newer titles, audio or transcript and are removed with the profile',async()=>{
 const profile=createProfile('Photo voice',new File(['unchanged audio'],'voice.wav'),{referenceText:'ጤና'});await saveProfile(profile);
 const first='data:image/jpeg;base64,/9j/2Q==',second='data:image/jpeg;base64,/9j/AAAA';
 await setProfileImage(profile.id,'/profile-images/default-female.jpg');assert.equal((await listProfiles()).find(item=>item.id===profile.id).image,'/profile-images/default-female.jpg');
 await setProfileImage(profile.id,first);await renameProfile(profile.id,'Renamed voice','Photo voice');
 const updated=await setProfileImage(profile.id,second);assert.equal(updated.name,'Renamed voice');assert.equal(updated.image,second);assert.equal(updated.referenceText,'ጤና');assert.equal(await updated.reference.text(),'unchanged audio');
 assert.equal((await listProfiles()).find(item=>item.id===profile.id).image,second);
 for(const invalid of ['https://example.com/photo.jpg','/profile-images/arbitrary.jpg','data:image/svg+xml;base64,AAAA','data:image/jpeg;base64,'+'A'.repeat(200_000)])await assert.rejects(setProfileImage(profile.id,invalid));
 assert.equal((await listProfiles()).find(item=>item.id===profile.id).image,second);
 await deleteProfile(profile.id);await assert.rejects(setProfileImage(profile.id,first),/deleted/);assert.equal((await listProfiles()).length,0);
});
