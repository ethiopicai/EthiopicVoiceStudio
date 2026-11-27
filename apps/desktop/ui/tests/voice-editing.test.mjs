import test from 'node:test';
import assert from 'node:assert/strict';
import {editingBlocker,normalizeNumbers,textWarnings,wordDiff,speechChanged} from '../src/voice-editing.js';
test('editing actions explain hardware, consent, setup, timing and no-change blockers',()=>{
  const ready={selection:{available:true,reason:'Ready'},consent:true,ready:true,runtimePresent:true};
  const edit={regenerate:true,before:'ሰላም ልጆች',after:'ሰላም ወዳጆች',hasWords:true};
  assert.equal(editingBlocker(ready,edit),null);
  assert.equal(editingBlocker(null,edit).code,'checking');
  assert.equal(editingBlocker(ready,{...edit,busy:true}).code,'busy');
  const apple={...ready,selection:{available:false,reason:'Voice Editing is not available on this ARM platform yet.'},ready:false};
  assert.deepEqual(editingBlocker(apple,edit),{code:'unavailable',message:apple.selection.reason});
  assert.equal(editingBlocker({...apple,selection:{...apple.selection,opt_in_required:true}},edit).code,'slow');
  assert.equal(editingBlocker({...ready,consent:false},edit).code,'consent');
  assert.equal(editingBlocker({...ready,ready:false,runtimePresent:false},edit).code,'setup');
  assert.equal(editingBlocker(ready,{...edit,hasWords:false}).code,'alignment');
  assert.equal(editingBlocker(ready,{...edit,after:edit.before+'።'}).code,'unchanged');
  assert.equal(editingBlocker(ready,{...edit,before:'ሐይል',after:'ሀይል'}).code,'unchanged');
  assert.equal(editingBlocker(ready,{...edit,after:''}),null,'removing all speech is an intentional supported edit');
});
test('voice editing diff preserves unchanged words and shows insertions/deletions',()=>{
  const d=wordDiff('ሰላም ዛሬ ነው','ሰላም ነገ ነው');
  assert.deepEqual(d,[{kind:'equal',text:'ሰላም'},{kind:'delete',text:'ዛሬ'},{kind:'insert',text:'ነገ'},{kind:'equal',text:'ነው'}]);
  assert.equal(speechChanged('ሰላም።','ሰላም!'),false);
  assert.equal(speechChanged('ሰላም',''),true);
  assert.equal(speechChanged('ሑለት ፁሑፍ','ሁለት ጹሁፍ'),false,'the model spelling table is shared with automatic speech preparation');
});
test('warnings and explicit normalization retain Latin text for human review',()=>{
  assert.deepEqual(textWarnings('12 dollars'),{digits:true,latin:true});
  assert.equal(normalizeNumbers('12 105 001 0.25 Alex'),'አስራ ሁለት አንድ መቶ አምስት ዜሮ ዜሮ አንድ ዜሮ ነጥብ ሁለት አምስት Alex');
  assert.deepEqual(textWarnings('ሰላም።'),{digits:false,latin:false});
  assert.equal(normalizeNumbers('١٢ １２'),'አስራ ሁለት አስራ ሁለት');
});
test('large transcript diff stays bounded and preserves both reconstructions',()=>{
  const before=Array.from({length:2000},(_,i)=>'old'+i).join(' '),after=Array.from({length:2000},(_,i)=>'new'+i).join(' ');
  const diff=wordDiff(before,after);
  assert.equal(diff.filter(p=>p.kind!=='insert').map(p=>p.text).join(' '),before);
  assert.equal(diff.filter(p=>p.kind!=='delete').map(p=>p.text).join(' '),after);
});
