import test from 'node:test';
import assert from 'node:assert/strict';
import {existsSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
import {preview} from 'vite';
import {chromium} from 'playwright-core';
import {seedDownloadReview} from './download-review-fixture.mjs';
const executable=process.env.CHROME_PATH||'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
test('first setup and every asset download wait for explicit permission; cancel, unknown and unavailable quotes never start setup',{skip:!existsSync(executable),timeout:60000},async()=>{
 const server=await preview({root:fileURLToPath(new URL('..',import.meta.url)),preview:{host:'127.0.0.1',port:0,open:false}}),browser=await chromium.launch({executablePath:executable,headless:true});
 try{
  const page=await browser.newPage({viewport:{width:1200,height:950}}),errors=[];page.on('pageerror',error=>errors.push(error.message));
  await page.addInitScript(()=>{
   window.setupState={state:'idle',stage:'Ready to set up.',downloadedBytes:0,totalBytes:120000000,canInstall:true};
   window.__TAURI_INTERNALS__={invoke:async(command)=>{
    if(command==='system_summary')return {os:'macos',arch:'aarch64',logicalCpus:4,totalMemoryBytes:8*1024**3};
    if(command==='get_voice_setup')return window.setupState;
    if(command==='start_voice_setup'){window.setupState.state='downloading';return;}
    if(command==='pause_voice_setup'){window.setupState.state='paused';return;}
    if(command==='get_asr_status'||command==='get_cleanup_status')return {state:'not_installed',message:'Not installed'};
    if(command.startsWith('get_active_'))return null;
    throw Error('Outside this test');
   }};
  });
  await page.addInitScript(seedDownloadReview,true);
  await page.goto(server.resolvedUrls.local[0]);
  const introduction=page.getByRole('dialog',{name:'Set up your voice tools',exact:true});await introduction.waitFor();
  assert.deepEqual(await page.evaluate(()=>window.downloadRequests),[]);
  await introduction.getByRole('button',{name:'Not now',exact:true}).click();
  assert.equal(await page.evaluate(()=>window.introductionSeen),true);
  await page.getByRole('button',{name:'Settings',exact:true}).click();
  const setup=page.getByRole('region',{name:'Voice tools setup',exact:true});
  await setup.getByRole('button',{name:'Download voice tools',exact:true}).click();
  const review=page.getByRole('dialog',{name:'Review your download',exact:true});
  await review.getByText('120.0 MB',{exact:true}).waitFor();
  await review.getByText('100.0 MB',{exact:true}).first().waitFor();await review.getByText('20.0 MB',{exact:true}).first().waitFor();
  await review.locator('summary').click();await review.getByText('amharic-model.bin',{exact:true}).waitFor();await review.getByText('speech-runtime.tar.gz',{exact:true}).waitFor();
  await page.screenshot({path:'/tmp/ethiopic-download-review.png'});
  assert.deepEqual(await page.evaluate(()=>window.downloadRequests),[]);
  await review.getByRole('button',{name:'Not now',exact:true}).click();assert.deepEqual(await page.evaluate(()=>window.downloadRequests),[]);
  await setup.getByRole('button',{name:'Download voice tools',exact:true}).click();await review.getByRole('button',{name:'Download and set up',exact:true}).click();
  await setup.getByRole('button',{name:'Pause download',exact:true}).waitFor();
  assert.deepEqual(await page.evaluate(()=>window.downloadRequests),[{command:'start_voice_setup',args:{approvedPlan:'quote-voice-tools',acceptedConsent:false}}]);
  await setup.getByRole('button',{name:'Pause download',exact:true}).click();
  await page.evaluate(()=>window.downloadAvailable=false);
  await setup.getByRole('button',{name:'Continue setup',exact:true}).click();await review.getByText('Compatible runtime has not been released.',{exact:true}).waitFor();
  assert.equal(await review.getByRole('button',{name:'Download and set up',exact:true}).isDisabled(),true);await review.getByRole('button',{name:'Not now',exact:true}).click();
  await page.evaluate(()=>window.downloadQuoteFails=true);await setup.getByRole('button',{name:'Continue setup',exact:true}).click();
  await review.getByRole('alert').waitFor();assert.equal(await review.getByRole('button',{name:'Download and set up',exact:true}).isDisabled(),true);
  assert.equal(await page.evaluate(()=>window.downloadRequests.length),1);await page.screenshot({path:'/tmp/ethiopic-download-review-error.png'});assert.deepEqual(errors,[]);
 }finally{await browser.close();await new Promise((resolve,reject)=>server.httpServer.close(error=>error?reject(error):resolve()));}
});
