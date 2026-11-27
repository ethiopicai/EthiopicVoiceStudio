import {seedDownloadReview} from './download-review-fixture.mjs';
import test from 'node:test';
import assert from 'node:assert/strict';
import {existsSync,readFileSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
import {preview} from 'vite';
import {chromium} from 'playwright-core';
import {seedVoiceHistory} from './voice-history-fixture.mjs';
const executable=process.env.CHROME_PATH||'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
test('Mac CPU generation has independent setup, default voice and transcript-free reference cloning',{skip:!existsSync(executable),timeout:60000},async()=>{
 const server=await preview({root:fileURLToPath(new URL('..',import.meta.url)),preview:{host:'127.0.0.1',port:0,open:false}}),browser=await chromium.launch({executablePath:executable,headless:true});
 try{
 const page=await browser.newPage({viewport:{width:1400,height:1050}}),errors=[];page.on('pageerror',e=>errors.push(e.message));
 await page.addInitScript(seedVoiceHistory,0);
 await page.addInitScript(()=>{if(window.self!==window.top)return;const base=window.__TAURI_INTERNALS__.invoke;window.synthesisInputs=[];window.compressedImports=[];window.genStatus={ready:false,consent:true,busy:false,stage:'',loaded:false,canPrepare:true,runtimePresent:true,provider:'tts-flow_lm-90m-amh-fp32',modelVersion:'v0.1',requiresReferenceText:false,defaultVoice:true,reason:'Download speech tools once.'};window.failGeneration=false;
 window.__TAURI_INTERNALS__.invoke=async(command,args={})=>{
  if(command==='resolve_recording_transfer'){window.compressedImports.push(args.input.name);return {name:args.input.name.replace(/\.mp3$/i,'.wav'),bytes:window.voiceBytes,context:{text:'',words:[],editedRegions:[],lineage:[]}};}
  if(command==='get_voice_generation_status')return structuredClone(window.genStatus);
  if(command==='get_voice_generation_activity')return {busy:window.genStatus.busy,stage:'Generating your speech…',loaded:true};
  if(command==='prepare_voice_generation'){window.genStatus.ready=true;window.genStatus.reason='Speech tools ready.';return;}
  if(command==='get_default_voice'){window.voiceCalls.push(command);return {name:'Default voice.wav',bytes:window.voiceBytes};}
  if(command==='synthesize_speech'){const input=args.input,project=await window.__TAURI_INTERNALS__.invoke('import_voice_edit_project',{...input,text:input.referenceText});await window.__TAURI_INTERNALS__.invoke('voice_edit_action',{...input,projectId:project.id});return {groupId:project.id,projectId:project.id};}
  if(command==='import_voice_edit_project'){window.synthesisInputs.push(structuredClone(args));return {id:`new-speech-${window.synthesisInputs.length}`,revision:0};}
  if(command==='voice_edit_action'){if(window.failGeneration)throw 'Reference voice could not be read.';const finish=()=>{window.genStatus.busy=false;window.voiceGenerations.push({id:args.projectId,name:args.projectId+'.wav',text:args.text,revision:0,created:2});return {sample_rate:24000,model:'tts-flow_lm-90m-amh-fp32',model_version:'v0.1'};};if(window.holdGeneration){window.genStatus.busy=true;return new Promise(resolve=>window.finishGeneration=()=>resolve(finish()));}return finish();}
  return base(command,args);
 };
 });
 await page.addInitScript(seedDownloadReview);
 await page.goto(server.resolvedUrls.local[0]);await page.locator('button.nav-item[aria-label="Text to Speech"]').click();
 await page.getByRole('button',{name:'Download speech tools',exact:true}).click();await page.getByRole('dialog',{name:'Review your download'}).getByRole('button',{name:'Download and set up',exact:true}).click();await page.getByText('Ready to generate speech on this computer.',{exact:true}).waitFor();
 await page.getByRole('textbox',{name:'Script',exact:true}).fill('ሰላም ጤና ይስጥልኝ።');await page.getByRole('button',{name:'Generate speech',exact:true}).click();await page.getByRole('button',{name:'Open generated speech: new-speech-1.wav',exact:true}).waitFor();
 assert.equal(await page.evaluate(()=>window.voiceCalls.filter(c=>c==='get_default_voice').length),1);assert.equal(await page.evaluate(()=>window.synthesisInputs[0].text),'');assert.equal(await page.evaluate(()=>window.voiceCalls.includes('get_voice_edit_status')),false,'TTS does not use editing readiness');
 await page.locator('button.nav-item[aria-label="Text to Speech"]').click();await page.getByRole('button',{name:'Manage voices',exact:true}).click();await page.getByRole('button',{name:'Open card: Voice one',exact:true}).click();
 const transcript=page.getByRole('textbox',{name:'Reference transcript (optional)',exact:true});await transcript.fill('');await page.getByRole('textbox',{name:'Script',exact:true}).fill('ዛሬ እንዴት ናችሁ።');await page.getByRole('button',{name:'Generate speech',exact:true}).click();await page.getByRole('button',{name:'Open generated speech: new-speech-2.wav',exact:true}).waitFor();
 assert.equal(await page.evaluate(()=>window.synthesisInputs[1].text),'');assert.equal(await page.evaluate(()=>window.voiceCalls.filter(c=>c==='get_default_voice').length),1,'reference cloning uses the chosen WAV rather than a preset');
 await page.evaluate(()=>window.failGeneration=true);await page.getByRole('button',{name:'Generate speech',exact:true}).click();await page.getByRole('alert').filter({hasText:'Reference voice could not be read.'}).waitFor();
 await page.evaluate(()=>{window.failGeneration=false;window.holdGeneration=true;});await page.getByRole('button',{name:'Generate speech',exact:true}).click();await page.waitForFunction(()=>!!window.finishGeneration);
 await page.locator('button.nav-item[aria-label="Home"]').click();await page.locator('button.nav-item[aria-label="Text to Speech"]').click();await page.getByRole('button',{name:'Cancel voice task',exact:true}).waitFor();
 await page.evaluate(()=>window.finishGeneration());await page.getByRole('button',{name:'Open generated speech: new-speech-4.wav',exact:true}).waitFor();
 await page.locator('button.nav-item[aria-label="Text to Speech"]').click();
 await page.getByLabel('Choose reference audio',{exact:true}).setInputFiles({name:'reference.MP3',mimeType:'audio/mpeg',buffer:readFileSync(new URL('../../src-tauri/tests/fixtures/tone.mp3',import.meta.url))});
 await page.getByText('reference.wav',{exact:true}).waitFor();
 await page.getByRole('textbox',{name:'Script',exact:true}).fill('ሰላም እንዴት ነህ።');
 await page.evaluate(()=>window.holdGeneration=false);
 await page.getByRole('button',{name:'Generate speech',exact:true}).click();await page.getByRole('button',{name:'Open generated speech: new-speech-5.wav',exact:true}).waitFor();
 assert.deepEqual(await page.evaluate(()=>window.compressedImports),['reference.MP3']);
 assert.equal(await page.evaluate(()=>window.synthesisInputs[4].name),'reference.wav','reference synthesis gets decoded WAV, including for existing PCM-only workers');
 assert.deepEqual(errors,[]);
 }finally{await browser.close();await new Promise((resolve,reject)=>server.httpServer.close(error=>error?reject(error):resolve()));}
});
