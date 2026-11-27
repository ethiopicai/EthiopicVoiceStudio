import test from 'node:test';
import assert from 'node:assert/strict';
import {generationBlocker} from '../src/voice-generation.js';
test('CPU default TTS and reference cloning work without editing eligibility or reference transcripts',()=>{
 const status={ready:true,consent:true,canPrepare:true,defaultVoice:true,requiresReferenceText:false},input={style:'preset:default',hasReference:false,referenceText:'',text:'ሰላም።'};
 assert.equal(generationBlocker(status,input),null);
 assert.equal(generationBlocker(status,{...input,style:'reference'}).code,'reference');
 assert.equal(generationBlocker(status,{...input,style:'reference',hasReference:true}),null);
 assert.equal(generationBlocker({...status,requiresReferenceText:true},{...input,hasReference:true}).code,'reference-text');
 assert.equal(generationBlocker(status,{...input,style:'preset:demo'}).code,'reference');
 assert.equal(generationBlocker(status,{...input,text:''}).code,'text');
 assert.equal(generationBlocker(status,{...input,text:'x'.repeat(12001)}).code,'length');
 assert.equal(generationBlocker(status,{...input,busy:true}).code,'busy');
 assert.equal(generationBlocker({...status,consent:false},input).code,'consent');
 assert.equal(generationBlocker({...status,ready:false},input).code,'setup');
 assert.deepEqual(generationBlocker({...status,ready:false,canPrepare:false,reason:'Runtime not published.'},input),{code:'unavailable',message:'Runtime not published.'});
 assert.equal(generationBlocker(null,input).code,'checking');
});
