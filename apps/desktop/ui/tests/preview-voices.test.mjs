import 'fake-indexeddb/auto';
import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {createHash} from 'node:crypto';
import {ensurePreviewVoices} from '../src/preview-voices.js';
import {listProfiles,renameProfile,deleteProfile,installPresetPack} from '../src/voice-profiles.js';

const publicRoot=new URL('../public/',import.meta.url);
const fileFetch=async path=>new Response(await readFile(new URL(path.slice(1),publicRoot)));

test('all 17 bundled references have verified PCM audio, exact script, provenance and 14 distinct personas',async()=>{
 const catalog=JSON.parse(await readFile(new URL('reference-voices/preview-v3/catalog.json',publicRoot),'utf8'));
 const legacy=JSON.parse(await readFile(new URL('../../../../scripts/reference-voices/preview-v1-catalog.json',import.meta.url),'utf8'));
 assert.equal(catalog.voices.length,17);assert.equal(new Set(catalog.voices.map(voice=>voice.personaId)).size,14);
 const exactText=(await readFile(new URL('../../../../scripts/reference-voices/preview-text.txt',import.meta.url),'utf8')).replace(/\r?\n$/, '');
 assert.equal(catalog.text,exactText);
 assert.ok(catalog.text.includes('ድምጽ ፕሮፋይል ይምረጡ ቀጥሎ'));
 assert.ok(catalog.text.includes('ወይም ኦፍላይን ሆነው'));
 for(const voice of catalog.voices){
  const bytes=await readFile(new URL(voice.audio.slice(1),publicRoot));
  assert.equal(createHash('sha256').update(bytes).digest('hex'),voice.audioSha256);
  assert.equal(bytes.length,voice.bytes);assert.equal(bytes.toString('ascii',0,4),'RIFF');assert.equal(bytes.toString('ascii',8,12),'WAVE');
  assert.equal(bytes.readUInt16LE(20),1);assert.equal(bytes.readUInt16LE(22),1);assert.equal(bytes.readUInt32LE(24),24000);assert.equal(bytes.readUInt16LE(34),16);
  assert.equal(voice.generation.baseSamplerDecodeSteps,1);assert.equal(voice.generation.samplerDecodeSteps,2);assert.equal(voice.generation.stepMultiplier,2);
  assert.equal(voice.referenceText,catalog.text);assert.equal(voice.license.noncommercial,true);assert.equal(voice.genderVerified,false);
  const portrait=await readFile(new URL(voice.image.slice(1),publicRoot));assert.ok(portrait.length>1000);assert.equal(portrait.readUInt16BE(0),0xffd8);
  assert.match(voice.source.sha256,/^[a-f0-9]{64}$/);assert.equal(voice.source.revision,'04d285138f0c2f8a7939dd9a3e48d8be12dc1712');
  assert.ok(voice.peak<.999&&voice.rms>.001);assert.ok(voice.durationSeconds>=legacy.voices.find(old=>old.id===voice.id).durationSeconds+12);
  assert.equal((bytes.length-44)/48000,voice.durationSeconds);assert.ok(voice.longerThanV1Seconds>=12);
  assert.match(voice.name,/[\u1200-\u137f]/);assert.doesNotMatch(voice.name,/RU|TR|EN|FR|DE|IT|NL|CS|AR|HU|HI|PT|PL|KO|ES|JA/);
  assert.equal(voice.generation.sourceConditioningMaxSeconds,5);assert.ok(voice.generation.sourceConditioningSeconds>0&&voice.generation.sourceConditioningSeconds<=5);
 }
 const portraits=await Promise.all([...new Set(catalog.voices.map(voice=>voice.image))].map(async path=>createHash('sha256').update(await readFile(new URL(path.slice(1),publicRoot))).digest('hex')));assert.equal(new Set(portraits).size,14);
});

test('a corrupted bundled recording installs no voices and leaves setup retryable',async()=>{
 let changed=false;
 const corrupt=async path=>{if(!changed&&path.endsWith('.wav')){changed=true;return new Response('bad audio');}return fileFetch(path);};
 await assert.rejects(ensurePreviewVoices(corrupt),/integrity/);assert.equal((await listProfiles()).length,0);
});

test('bundled preset installation is atomic and idempotent, preserving renamed and deleted voices',async()=>{
 let count=0;const fetcher=async path=>{count++;return fileFetch(path);};
 await Promise.all([ensurePreviewVoices(fetcher),ensurePreviewVoices(fetcher)]);
 const voices=await listProfiles();assert.equal(voices.length,17);assert.equal(new Set(voices.map(voice=>voice.id)).size,17);
 assert.equal(await voices[0].reference.arrayBuffer().then(bytes=>bytes.byteLength),voices[0].reference.size);
 await renameProfile(voices[0].id,'My renamed reference',voices[0].name);await deleteProfile(voices[1].id);
 const previousCount=count;await ensurePreviewVoices(fetcher);assert.equal(count,previousCount);
 const updated=await listProfiles();assert.equal(updated.length,16);assert.equal(updated.find(voice=>voice.id===voices[0].id).name,'My renamed reference');assert.ok(!updated.some(voice=>voice.id===voices[1].id));
});

test('a failed pack load leaves no partial profiles or success receipt',async()=>{
 await assert.rejects(installPresetPack('broken-pack',async()=>{throw Error('download interrupted');}),/interrupted/);
 let called=0;await assert.rejects(installPresetPack('broken-pack',async()=>{called++;return [];}),/incomplete/);assert.equal(called,1);
 assert.equal((await listProfiles()).length,16);
});
