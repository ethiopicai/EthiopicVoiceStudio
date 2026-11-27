import test from 'node:test';
import assert from 'node:assert/strict';
import {speechCapabilities,defaultSpeechControls,speechControlError,speechGenerationRequest,savedSpeechSettings} from '../src/speech-controls.js';
const cpu=speechCapabilities({provider:'tts-flow_lm-90m-amh-fp32'}),gpu=speechCapabilities({provider:'tts-codec_llm-0.6b-amh-fp16'});
test('CPU and GPU requests include only controls that their engine supports',()=>{
 const c=defaultSpeechControls(cpu),g=defaultSpeechControls(gpu);
 assert.equal(c.temperature,.3);assert.equal(g.temperature,0);assert.equal(g.speed,1);assert.equal(g.numSteps,32);
 assert.deepEqual(speechGenerationRequest(c,cpu,()=>72),{seed:72,generationOptions:{temperature:.3,sentencePause:.2}});
 assert.deepEqual(speechGenerationRequest({...g,automaticSeed:false,seed:5,speed:.9,numSteps:64},gpu),{seed:5,generationOptions:{temperature:0,sentencePause:.2,speed:.9,numSteps:64}});
 assert.equal(speechCapabilities({provider:'tts-flow_lm-90m-amh-fp32',controls:{temperature:false}}).temperature,false);
 assert.equal(speechCapabilities({provider:'tts-codec_llm-0.6b-amh-fp16',controls:{temperature:false,seed:true}}).speed,false,'an older native capability report does not silently gain unsupported fields');
});
test('target duration replaces speed and sentence pauses rather than silently ignoring controls',()=>{
 const values={...defaultSpeechControls(gpu),useDuration:true,duration:4,speed:undefined,sentencePause:undefined};
 assert.deepEqual(speechGenerationRequest(values,gpu,()=>7),{seed:7,generationOptions:{temperature:0,numSteps:32,duration:4}});
 assert.equal(speechControlError(values,gpu),'');
 assert.ok(speechControlError({...values,duration:0},gpu));
});
test('invalid and empty numeric controls block generation',()=>{
 for(const value of [undefined,NaN,Infinity,'0.3',0,1.1])assert.ok(speechControlError({...defaultSpeechControls(cpu),temperature:value},cpu));
 for(const [key,bad] of [['speed',.4],['speed',2.1],['numSteps',8],['numSteps',17],['sentencePause',-1]])assert.ok(speechControlError({...defaultSpeechControls(gpu),[key]:bad},gpu));
 for(const seed of [undefined,NaN,-1,4294967296,1.2])assert.throws(()=>speechGenerationRequest({...defaultSpeechControls(cpu),automaticSeed:false,seed},cpu));
 assert.throws(()=>speechGenerationRequest(defaultSpeechControls(cpu),cpu,()=>NaN));
 assert.equal(speechControlError({...defaultSpeechControls(cpu),temperature:undefined,sentencePause:undefined},speechCapabilities(null)),'');
});
test('saved settings restore valid actual controls and keep old seed-only recordings compatible',()=>{
 for(const value of [null,{}, {schemaVersion:1,seed:-1},{schemaVersion:2,seed:2}])assert.equal(savedSpeechSettings(value),null);
 assert.deepEqual(savedSpeechSettings({schemaVersion:1,seed:0}),{schemaVersion:1,seed:0});
 assert.deepEqual(savedSpeechSettings({schemaVersion:1,temperature:0,speed:.8,numSteps:16,seed:99,sentencePause:.5}),{schemaVersion:1,temperature:0,speed:.8,numSteps:16,seed:99,sentencePause:.5});
 assert.deepEqual(savedSpeechSettings({schemaVersion:1,duration:4,speed:1,sentencePause:1,seed:5}),{schemaVersion:1,duration:4,seed:5});
});
