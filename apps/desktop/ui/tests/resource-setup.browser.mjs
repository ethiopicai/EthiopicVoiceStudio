import {seedDownloadReview} from './download-review-fixture.mjs';
import test from 'node:test';
import assert from 'node:assert/strict';
import {existsSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
import {preview} from 'vite';
import {chromium} from 'playwright-core';
const executable=process.env.CHROME_PATH||'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
for(const os of ['macos','windows'])test(`${os}: full setup waits for speech inference and refreshes tools automatically`,{skip:!existsSync(executable)},async()=>{
 const server=await preview({root:fileURLToPath(new URL('..',import.meta.url)),preview:{host:'127.0.0.1',port:0,open:false}}),browser=await chromium.launch({executablePath:executable,headless:true});
 try{
  const page=await browser.newPage({viewport:{width:1300,height:1000}});
  await page.addInitScript(os=>{
   window.setup={state:'idle',stage:'Set up your voice tools.',downloadedBytes:0,totalBytes:100,canInstall:true,ready:false,requiresConsent:true,step:0};
   window.changeSetup=update=>{Object.assign(window.setup,update);window.dispatchEvent(new Event('studio-tools-changed'));};
   window.hardwareChecks=0;window.recognitionReady=false;
   window.__TAURI_INTERNALS__={invoke:async(command,args)=>{
    if(command==='get_voice_setup')return {...window.setup,steps:['Download voice tools','Prepare them','Check they work'].map((title,index)=>({id:String(index),title,state:window.setup.ready||index<window.setup.step?'done':index>window.setup.step?'pending':['paused','failed'].includes(window.setup.state)?window.setup.state:window.setup.state==='idle'?'pending':'active'}))};
    if(command==='start_voice_setup'){window.setup.state='downloading';window.setup.requiresConsent=false;return;}
    if(command==='pause_voice_setup'){window.setup.state='paused';return;}
    if(command==='get_setup_plan'){window.hardwareChecks++;return {supported:true,reason:'Matched to this computer.',hardware:{os,arch:os==='windows'?'x86_64':'aarch64',totalRamBytes:8*1024**3,freeRamBytes:4*1024**3,physicalCores:4,nvidia:[]},osVersion:os==='windows'?'11':'14.0',freeStorageBytes:100*1024**3,routing:{tier:'cpu-high',downloadBytes:100,downloads:[],warnings:[]},features:['Transcription','Dictation','Text to Speech'].map((title,index)=>({id:['transcription','dictation','speech'][index],title,model:null,downloads:[],modelBytes:0,ready:window.setup.ready,canInstall:true,requiresConsent:false,requiredStorageBytes:0,message:window.setup.ready?'Ready to use.':'Setup available.'}))};}
    if(command==='get_asr_runtime_status')return {keepReady:false,state:'unloaded'};
    if(command==='get_asr_status')return {state:window.recognitionReady?'ready':'not_installed',message:'Recognition status'};
    if(command==='get_cleanup_status')return {state:'not_installed',message:'Not installed'};
    if(command.startsWith('get_active_'))return null;
    if(command==='system_summary')return {os,arch:os==='windows'?'x86_64':'aarch64',logicalCpus:4,totalMemoryBytes:8*1024**3};
    throw Error('Outside this test');
   }};
  },os);
  await page.addInitScript(seedDownloadReview);await page.goto(server.resolvedUrls.local[0]);
  const setup=page.getByRole('region',{name:'Voice tools setup',exact:true});await setup.waitFor();
  const button=setup.getByRole('button',{name:'Download voice tools',exact:true});assert.equal(await button.isDisabled(),true);
  await setup.getByRole('checkbox').check();await button.click();const review=page.getByRole('dialog',{name:'Review your download'});await review.getByText('Maximum download',{exact:true}).waitFor();
  assert.deepEqual(await page.evaluate(()=>window.downloadRequests),[]);await review.getByRole('button',{name:'Download and set up',exact:true}).click();
  assert.deepEqual(await page.evaluate(()=>window.downloadRequests),[{command:'start_voice_setup',args:{approvedPlan:'quote-voice-tools',acceptedConsent:true}}]);
  await setup.getByRole('button',{name:'Pause download',exact:true}).click();await setup.getByRole('button',{name:'Continue setup',exact:true}).waitFor();
  await page.evaluate(()=>window.changeSetup({state:'failed',downloadedBytes:48,error:'Connection interrupted. Progress is saved.'}));await setup.getByRole('alert').filter({hasText:'Progress is saved'}).waitFor();
  await setup.getByRole('button',{name:'Continue setup',exact:true}).click();await review.getByRole('button',{name:'Download and set up',exact:true}).click();
  await page.evaluate(()=>{window.recognitionReady=true;window.changeSetup({state:'preparing',step:1,downloadedBytes:100,error:null,stage:'Preparing speech tools…'});});await setup.getByText('Preparing speech tools…',{exact:true}).waitFor();
  assert.equal(await setup.locator('.setup-steps .done').count(),1);assert.equal(await setup.getByRole('heading',{name:'Your voice tools are ready',exact:true}).count(),0);
  await page.evaluate(()=>window.changeSetup({state:'checking',step:2,stage:'Trying a short recording and a little speech…'}));await setup.getByText('Trying a short recording and a little speech…',{exact:true}).waitFor();
  assert.equal(await setup.locator('.setup-steps .done').count(),2);assert.equal(await setup.getByRole('progressbar').getAttribute('value'),null,'preparation/checks use an indeterminate bar rather than a stuck download percentage');
  const before=await page.evaluate(()=>window.hardwareChecks);await page.evaluate(()=>window.changeSetup({state:'installed',step:3,ready:true,stage:'Ready! Work offline.'}));
  await setup.getByRole('heading',{name:'Your voice tools are ready',exact:true}).waitFor();assert.equal(await setup.locator('.setup-steps .done').count(),3);
  await page.waitForFunction(()=>document.querySelectorAll('.matched-tools .ready').length===3);assert.ok(await page.evaluate(()=>window.hardwareChecks)>before,'completion refreshes tools without another click');
  assert.equal(await page.getByRole('region',{name:'Automatic model setup',exact:true}).getByText('Setup available.',{exact:true}).count(),0);
  await page.screenshot({path:`/tmp/ethiopic-guided-setup-${os}.png`});await page.getByRole('button',{name:'Home',exact:true}).click();assert.equal(await page.getByRole('region',{name:'Voice tools setup shortcut'}).count(),0);
 }finally{await browser.close();await new Promise(r=>server.httpServer.close(r));}
});
