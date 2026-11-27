import test from 'node:test';
import assert from 'node:assert/strict';
import {shortcutFromKey,shortcutLabel,canSaveShortcut} from '../src/shortcut-input.js';
const event=(code,more={})=>({code,ctrlKey:false,altKey:false,shiftKey:false,metaKey:false,repeat:false,isComposing:false,...more});
test('Windows Ctrl + Win is captured as a modifier-only hold',()=>{
  const shortcut=shortcutFromKey(event('MetaLeft',{ctrlKey:true,metaKey:true}));
  assert.deepEqual(shortcut,{modifiers:9,key:null});
  assert.equal(shortcutLabel(shortcut,'windows'),'Ctrl + Win');
  assert.equal(canSaveShortcut(shortcut,'windows'),true);
});
test('custom combinations distinguish ordinary typing and ignore repeats/composition',()=>{
  assert.equal(canSaveShortcut(shortcutFromKey(event('KeyD')),'windows'),false);
  assert.equal(canSaveShortcut(shortcutFromKey(event('ControlLeft',{ctrlKey:true})),'windows'),false);
  assert.deepEqual(shortcutFromKey(event('KeyD',{ctrlKey:true,shiftKey:true})),{modifiers:5,key:'KeyD'});
  for(const e of [event('KeyD',{repeat:true}),event('KeyD',{isComposing:true}),event('Escape'),event('Delete')])assert.equal(shortcutFromKey(e),null);
  assert.equal(shortcutLabel({modifiers:10,key:'Space'},'macos'),'Option + Command + Space');
  assert.equal(canSaveShortcut({modifiers:0,key:'Fn'},'macos'),true);
  assert.equal(canSaveShortcut({modifiers:0,key:'Fn'},'windows'),false);
});
