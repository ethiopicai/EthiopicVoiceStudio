import test from 'node:test';
import assert from 'node:assert/strict';
import {existsSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
import {preview} from 'vite';
import {chromium} from 'playwright-core';
import {seedVoiceHistory} from './voice-history-fixture.mjs';
const executable=process.env.CHROME_PATH||'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
test('recording titles update cards and history across tasks, survive reopening and preserve source filenames',{skip:!existsSync(executable),timeout:120000},async()=>{
 const server=await preview({root:fileURLToPath(new URL('..',import.meta.url)),preview:{host:'127.0.0.1',port:0,open:false}}),browser=await chromium.launch({executablePath:executable,headless:true});
 try{
  for(const [view,historyName,cardId,original] of [
   ['Transcription','Transcription history','transcript-transcript-one','Capture.wav'],
   ['Dictation','Transcription history','transcript-transcript-one','Capture.wav'],
   ['Transcription','Transcription history','draft-pending-one','Pending.wav'],
   ['Text to Speech','Voice history','voice-card-voice-project-generation-one','Generated one.wav'],
   ['Text to Speech','Reference voices','voice-card-voice-profile-profile-one','Voice one'],
   ['Voice Editing','Editing history','voice-card-editing-edit-one','Edit one.wav'],
   ['Voice Cleanup','Cleanup history','voice-card-cleanup-recording-input-one','Original one.wav'],
   ['Voice Cleanup','Cleanup history','voice-card-cleanup-result-result-one','Cleaned one.wav']
  ]){
   const page=await browser.newPage({viewport:{width:1400,height:1000},colorScheme:'dark'}),errors=[];page.on('pageerror',error=>errors.push(error.message));await page.addInitScript(seedVoiceHistory,0);
   await page.addInitScript(()=>{if(window.self!==window.top)return;const base=window.__TAURI_INTERNALS__.invoke;window.renameCalls=[];window.failRename=false;window.customTitles=JSON.parse(localStorage.getItem('test-recording-titles')||'{}');window.savedCaptionTranscript={id:'transcript-one',name:'Capture.wav',revision:1,text:'ሰላም',words:[],corrections:[],model_version:'v0.2',created_at_ms:1};
    window.__TAURI_INTERNALS__.invoke=async(command,args={})=>{
     if(command==='list_recording_titles')return structuredClone(window.customTitles);
     if(command==='rename_recording'){window.renameCalls.push(structuredClone(args));if(window.failRename)throw 'The title could not be saved. Retry.';const key=args.kind+':'+args.id;if((window.customTitles[key]??null)!==args.expectedTitle)throw 'Title changed elsewhere';window.customTitles[key]=args.title.trim();localStorage.setItem('test-recording-titles',JSON.stringify(window.customTitles));return window.customTitles[key];}
     if(command==='list_transcripts')return [structuredClone(window.savedCaptionTranscript)];
     if(command==='list_transcription_recordings')return [{id:'pending-one',name:'Pending.wav'}];
     if(command==='get_active_transcription_job')return null;
     if(command==='get_transcript')return structuredClone(window.savedCaptionTranscript);
     if(command==='get_transcript_audio')return {name:'Capture.wav',bytes:window.voiceBytes};
     return base(command,args);
    };
   });
   const nav=label=>page.locator(`button.nav-item[aria-label="${label}"]`);await page.goto(server.resolvedUrls.local[0]);await nav(view).click();
   const history=page.getByRole('region',{name:historyName,exact:true}),card=page.locator(`[id="${cardId}"]`);
   if(cardId.includes('voice-profile'))await page.locator('#voice-library > summary').click();
   const rename=card.getByRole('button',{name:`Rename ${original}`,exact:true});await rename.waitFor();
   await rename.click();let dialog=page.getByRole('dialog',{name:'Rename recording',exact:true});await dialog.getByLabel('Card title',{exact:true}).fill('Cancelled title');await page.keyboard.press('Escape');await dialog.waitFor({state:'hidden'});assert.equal(await history.getByText('Cancelled title',{exact:true}).count(),0);
   await rename.click();dialog=page.getByRole('dialog',{name:'Rename recording',exact:true});const title='የፖድካስት መግቢያ <intro>';
   await dialog.getByLabel('Card title',{exact:true}).fill(' ');assert.equal(await dialog.getByRole('button',{name:'Save title',exact:true}).isDisabled(),true);
   await dialog.getByLabel('Card title',{exact:true}).fill(title);
   if(view==='Transcription'&&cardId.startsWith('transcript-')){await page.evaluate(()=>window.failRename=true);await dialog.getByRole('button',{name:'Save title',exact:true}).click();await dialog.getByRole('alert').waitFor();assert.equal(await history.getByText(title,{exact:true}).count(),0);await page.evaluate(()=>window.failRename=false);}
   await dialog.getByRole('button',{name:'Save title',exact:true}).click();await dialog.waitFor({state:'hidden'});await (cardId.includes('voice-profile')?card.locator('.card-heading strong'):history.getByText(title,{exact:true})).waitFor();
   assert.ok((await card.innerText()).includes(title));
   assert.equal(await page.evaluate(()=>window.savedCaptionTranscript.name),'Capture.wav');assert.equal(await page.evaluate(()=>window.voiceGenerations[0].name),'Generated one.wav');
   await page.reload();await nav(view).click();if(cardId.includes('voice-profile'))await page.locator('#voice-library > summary').click();await (cardId.includes('voice-profile')?card.locator('.card-heading strong'):page.getByRole('region',{name:historyName,exact:true}).getByText(title,{exact:true})).waitFor();
   if(view==='Dictation'){await nav('Transcription').click();await page.getByRole('region',{name:'Transcription history',exact:true}).getByText(title,{exact:true}).waitFor();}
   if(cardId.includes('voice-profile')){
    await page.getByRole('button',{name:`Open card: ${title}`,exact:true}).click();const reference=await page.evaluate(async()=>{const db=await new Promise(resolve=>{const request=indexedDB.open('ethiopic-voice-profiles');request.onsuccess=()=>resolve(request.result);});const saved=await new Promise(resolve=>{const request=db.transaction('profiles','readonly').objectStore('profiles').get('profile-one');request.onsuccess=()=>resolve(request.result);});db.close();return {name:saved.reference.name,bytes:(await saved.reference.arrayBuffer()).byteLength};});assert.equal(reference.name,'one.wav');assert.equal(reference.bytes,32044);
   }
   await page.screenshot({path:`/tmp/ethiopic-renamed-${cardId}.png`});assert.deepEqual(errors,[]);await page.close();
  }
 }finally{await browser.close();await new Promise((resolve,reject)=>server.httpServer.close(error=>error?reject(error):resolve()));}
});
