import test from 'node:test';
import assert from 'node:assert/strict';
import {recordingSelectionKey,toggleRecordingSelection,pruneRecordingSelection,selectedRecordingItems} from '../src/recording-selection.js';
test('selection distinguishes drafts from transcripts and never mutates its input',()=>{
 const original=[];const selected=toggleRecordingSelection(original,'transcript','same',true);
 assert.deepEqual(original,[]);assert.deepEqual(toggleRecordingSelection(selected,'transcript','same',true),['transcript:same']);
 const both=toggleRecordingSelection(selected,'recording','same',true);assert.equal(both.length,2);
 assert.deepEqual(toggleRecordingSelection(both,'transcript','same',false),[recordingSelectionKey('recording','same')]);
});
test('confirmation snapshots revisions and removes stale selection identities',()=>{
 const entries=[{kind:'transcript',id:'a',revision:2,name:'private filename'},{kind:'recording',id:'b',name:'draft'}];
 const keys=['transcript:a','recording:b','transcript:missing'];
 assert.deepEqual(pruneRecordingSelection(keys,entries),['transcript:a','recording:b']);
 const items=selectedRecordingItems(keys,entries);entries[0].revision=3;
 assert.deepEqual(items,[{kind:'transcript',id:'a',revision:2},{kind:'recording',id:'b'}]);
 assert.equal('name' in items[0],false);
});
