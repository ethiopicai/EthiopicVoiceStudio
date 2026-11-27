import test from 'node:test';
import assert from 'node:assert/strict';
import {get} from 'svelte/store';
import {recordingTitles,recordingTitleError,displayRecordingTitle,refreshRecordingTitles,renameRecording,forgetRecordingTitles} from '../src/recording-titles.js';
test('recording titles accept Amharic text and remain separate from filenames',()=>{
 assert.equal(recordingTitleError('  የፖድካስት መግቢያ / intro  '),'');assert.equal(recordingTitleError('ሀ'.repeat(120)),'');
 for(const title of [' ','bad\nname','bad\u0080name','ሀ'.repeat(121)])assert.ok(recordingTitleError(title));
 assert.equal(displayRecordingTitle({'transcript:one':'Podcast introduction'},'transcript','one','capture.wav'),'Podcast introduction');assert.equal(displayRecordingTitle({},'transcript','one','capture.wav'),'capture.wav');
});
test('title persistence, stale loads and deletion do not replace newer titles or revive deleted entries',async()=>{
 const before=globalThis.window;let release;recordingTitles.set({});
 globalThis.window={__TAURI_INTERNALS__:{invoke:async(command,args)=>{if(command==='list_recording_titles')return new Promise(resolve=>release=resolve);assert.equal(command,'rename_recording');assert.equal(args.expectedTitle,null);assert.equal(args.title,'New title');return args.title;}}};
 try{
  const stale=refreshRecordingTitles();await renameRecording('transcript','one','  New title  ');release({'transcript:one':'Old title'});await stale;assert.equal(get(recordingTitles)['transcript:one'],'New title');
  const staleDelete=refreshRecordingTitles();forgetRecordingTitles([{kind:'transcript',id:'one'}]);release({'transcript:one':'New title'});await staleDelete;assert.equal(get(recordingTitles)['transcript:one'],undefined);
 }finally{recordingTitles.set({});if(before===undefined)delete globalThis.window;else globalThis.window=before;}
});
