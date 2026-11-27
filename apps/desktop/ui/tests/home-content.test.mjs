import test from 'node:test';
import assert from 'node:assert/strict';
import {loadHomeContent,parseHomeContent,placeholderHomeProvider} from '../src/home-content.js';
test('bundled Home placeholder has all content areas and internal actions',async()=>{
  const {content,fallback}=await loadHomeContent();assert.equal(fallback,false);
  assert.deepEqual(content.sections.map(section=>section.kind),['personalized','tip','news']);
});
test('provider changes content without changing the page contract',async()=>{
  const payload=await placeholderHomeProvider();payload.sections[0].cards[0].title='A new recommendation';
  const {content}=await loadHomeContent(async()=>payload);
  assert.equal(content.sections[0].cards[0].title,'A new recommendation');
  assert.notEqual((await loadHomeContent()).content.sections[0].cards[0].title,'A new recommendation');
});
test('legacy clone actions open the unified Text to Speech workspace',async()=>{
  const payload=await placeholderHomeProvider();payload.sections[0].cards[0].action.target='clone';
  assert.equal(parseHomeContent(payload).sections[0].cards[0].action.target,'tts');
});
test('invalid or failed content falls back without blocking Home',async()=>{
  for(const provider of [async()=>{throw new Error('offline');},async()=>({schemaVersion:2}),async()=>null]){
    const result=await loadHomeContent(provider);assert.equal(result.fallback,true);assert.equal(result.content.schemaVersion,1);
  }
});
test('remote contract rejects arbitrary navigation, duplicate identities, and oversized copy',async()=>{
  for(const mutate of [value=>value.sections[0].cards[0].action.target='https://untrusted.example',value=>value.sections.push(value.sections[0]),value=>value.sections[0].cards[0].body='x'.repeat(601)]){
    const payload=await placeholderHomeProvider();mutate(payload);assert.throws(()=>parseHomeContent(payload));
  }
  const payload=await placeholderHomeProvider();payload.sections[0].cards[0].html='<script>run()</script>';
  assert.equal('html' in parseHomeContent(payload).sections[0].cards[0],false);
});
