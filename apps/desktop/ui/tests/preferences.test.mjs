import test from 'node:test';
import assert from 'node:assert/strict';
import {get} from 'svelte/store';
import {PREFERENCES_KEY,preferences,readPreferences,savePreferences,microphoneConstraints,validatePreferences} from '../src/preferences.js';
import {dictationStartupMessage} from '../src/dictation-startup.js';

test('settings survive serialization and unavailable storage keeps the previous choice',()=>{
  let saved;
  const storage={setItem:(key,value)=>{assert.equal(key,PREFERENCES_KEY);saved=value;},getItem:()=>saved};
  savePreferences({theme:'dark',microphoneId:'usb-input'},storage);
  assert.deepEqual(readPreferences(storage),{theme:'dark',microphoneId:'usb-input'});
  assert.throws(()=>savePreferences({theme:'light'},{setItem:()=>{throw Error('disk full');}}));
  assert.equal(get(preferences).theme,'dark');
  assert.throws(()=>savePreferences({theme:'light'},null));
  assert.deepEqual(readPreferences({getItem:()=>'{broken'}),{theme:'system',microphoneId:''});
  for(const bad of [{theme:'blue',microphoneId:''},{theme:'dark',microphoneId:'\n'},null])assert.throws(()=>validatePreferences(bad));
});
test('microphone selection is exact and system default leaves device selection to the OS',()=>{
  assert.equal(microphoneConstraints('').audio.deviceId,undefined);
  assert.deepEqual(microphoneConstraints('usb-input').audio.deviceId,{exact:'usb-input'});
  assert.equal(microphoneConstraints('usb-input').audio.channelCount,1);
});
test('startup errors identify the failing stage and retain controlled native reasons',()=>{
  for(const [name,match] of [['NotAllowedError',/Microphone access was not allowed/],['NotFoundError',/No microphone/],['OverconstrainedError',/selected microphone is unavailable/],['NotReadableError',/could not be opened/]])assert.match(dictationStartupMessage({name},'microphone'),match);
  assert.equal(dictationStartupMessage('Another voice task is running. Try again when it finishes.','voice-tools'),'Another voice task is running. Try again when it finishes.');
  assert.match(dictationStartupMessage(new Error('Startup needs more available memory.'),'voice-tools'),/more available memory/);
  assert.doesNotMatch(dictationStartupMessage('/Users/private/model/file','voice-tools'),/Users/);
  assert.match(dictationStartupMessage(new Error('opaque'),'capture'),/audio capture could not start/);
  assert.match(dictationStartupMessage(new Error('opaque'),'audio'),/input and output devices/);
});
