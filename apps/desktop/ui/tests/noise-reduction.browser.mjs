import {seedDownloadReview} from './download-review-fixture.mjs';
import test from 'node:test';
import assert from 'node:assert/strict';
import {existsSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
import {preview} from 'vite';
import {chromium} from 'playwright-core';
import {seedDictation} from './dictation-fixture.mjs';
import {observeCaptureSamples} from './audio-capture-fixture.mjs';
const executable=process.env.CHROME_PATH||'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
test('optional noise reduction shares setup/preference across tasks and preserves a 48 kHz recording clock',{skip:!existsSync(executable),timeout:45000},async()=>{
 const server=await preview({root:fileURLToPath(new URL('..',import.meta.url)),preview:{host:'127.0.0.1',port:0,open:false}}),browser=await chromium.launch({executablePath:executable,headless:true,args:['--use-fake-device-for-media-stream','--use-fake-ui-for-media-stream']});
 try{
  const page=await browser.newPage({viewport:{width:1300,height:950}});await page.addInitScript(seedDictation,0);
  await page.addInitScript(observeCaptureSamples);
  await page.addInitScript(()=>{
   window.noise={state:'not_installed',supported:true,ready:false,enabled:false,stage:'Optional download',received:0,total:235046335,device:null,liveReady:false};
   window.noiseRequests=[];window.noiseImports=[];
   const base=window.__TAURI_INTERNALS__.invoke;
   window.__TAURI_INTERNALS__.invoke=async(command,args)=>{
    if(command==='get_noise_reduction_status')return structuredClone(window.noise);
    if(command==='start_noise_reduction_setup'){window.noiseRequests.push(command);window.noise={...window.noise,state:'installing',stage:'Downloading noise reduction…',received:90000000};return structuredClone(window.noise);}
    if(command==='pause_noise_reduction_setup'){window.noise.state='paused';return;}
    if(command==='set_noise_reduction_enabled'){window.noiseRequests.push({command,...args});window.noise.enabled=args.enabled;return structuredClone(window.noise);}
    if(command==='start_dictation'){const session=await base(command,args);session.inputSampleRate=window.noise.enabled?48000:16000;return session;}
    if(command==='import_transcription_audio'){window.noiseImports.push(args.bytes);return base(command,args);}
    return base(command,args);
   };
  });
  await page.addInitScript(seedDownloadReview);
 await page.goto(server.resolvedUrls.local[0]);await page.locator('button.nav-item[aria-label="Transcription"]').click();
  const control=page.getByRole('region',{name:'Recognition noise reduction',exact:true}),checkbox=control.getByRole('checkbox',{name:'Reduce background noise',exact:true});
  await checkbox.waitFor();assert.equal(await checkbox.isChecked(),false);assert.equal(await checkbox.isDisabled(),true);
  await control.getByRole('button',{name:/Set up/}).click();await page.getByRole('dialog',{name:'Review your download'}).getByRole('button',{name:'Download and set up',exact:true}).click();await control.getByRole('progressbar',{name:'Noise reduction setup'}).waitFor();
  assert.equal(await control.getByRole('progressbar').getAttribute('value'),String(90000000/235046335));await control.getByRole('button',{name:'Pause setup',exact:true}).click();await control.getByRole('button',{name:'Resume setup',exact:true}).waitFor();
  await page.evaluate(()=>window.noise={...window.noise,state:'ready',stage:'Ready',ready:true,device:'mps',liveReady:true});
  await page.waitForFunction(()=>!document.querySelector('.noise-control input').disabled);await checkbox.check();
  await page.locator('button.nav-item[aria-label="Dictation"]').click();await page.getByRole('button',{name:'Recording settings',exact:true}).click();
  const dictationControl=page.getByRole('region',{name:'Recognition noise reduction',exact:true});await dictationControl.getByRole('checkbox',{name:'Reduce background noise',exact:true}).waitFor();assert.equal(await dictationControl.getByRole('checkbox').isChecked(),true);
  await page.getByRole('button',{name:'Start recording',exact:true}).click();await page.getByRole('button',{name:'Stop recording',exact:true}).waitFor();
  assert.equal(await dictationControl.getByRole('checkbox').isDisabled(),true);await page.waitForFunction(()=>window.totalSamples>16000);assert.equal(await page.evaluate(()=>window.session.inputSampleRate),48000);
  await page.getByRole('button',{name:'Stop recording',exact:true}).click();await page.locator('[data-transcript-id="live-1"]').waitFor();
  assert.ok(await page.evaluate(()=>window.totalSamples>16000));
  await page.getByRole('checkbox',{name:'Transcribe after stopping',exact:true}).check();
  await page.getByRole('button',{name:'Start recording',exact:true}).click();await page.getByRole('button',{name:'Stop recording',exact:true}).waitFor();await page.waitForFunction(()=>window.capturedSamples>2048);await page.getByRole('button',{name:'Stop recording',exact:true}).click();
  await page.waitForFunction(()=>window.noiseImports.length>0);assert.equal(await page.evaluate(()=>new DataView(Uint8Array.from(window.noiseImports[0]).buffer).getUint32(24,true)),48000);
  await page.locator('[data-transcript-id="batch-2"]').waitFor();await dictationControl.getByRole('checkbox').uncheck();
  await page.locator('button.nav-item[aria-label="Transcription"]').click();await checkbox.waitFor();assert.equal(await checkbox.isChecked(),false);
  assert.equal(await page.evaluate(()=>window.noiseRequests.filter(v=>typeof v==='object').length),2);
 }finally{await browser.close();await new Promise((resolve,reject)=>server.httpServer.close(error=>error?reject(error):resolve()));}
});
