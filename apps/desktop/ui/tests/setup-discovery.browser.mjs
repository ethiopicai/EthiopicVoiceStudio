import test from 'node:test';
import assert from 'node:assert/strict';
import {existsSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
import {preview} from 'vite';
import {chromium} from 'playwright-core';
import {seedVoiceHistory} from './voice-history-fixture.mjs';
import {seedDownloadReview} from './download-review-fixture.mjs';

const executable=process.env.CHROME_PATH||'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
async function openFixture({first=false,state='idle',ready=false}={}) {
  const server=await preview({root:fileURLToPath(new URL('..',import.meta.url)),preview:{host:'127.0.0.1',port:0,open:false}});
  const browser=await chromium.launch({executablePath:executable,headless:true});
  const page=await browser.newPage({viewport:{width:1300,height:950}});
  const errors=[];page.on('pageerror',error=>errors.push(error.message));
  await page.addInitScript(seedVoiceHistory,0);
  await page.addInitScript(({state,ready})=>{
    if(window.self!==window.top)return;
    const base=window.__TAURI_INTERNALS__.invoke;
    window.toolsReady=ready;
    window.changeSetup=update=>{Object.assign(window.setupState,update);window.dispatchEvent(new Event('studio-tools-changed'));};
    window.setupState={state,ready,requiresConsent:false,steps:[],stage:state==='paused'?'Your download is saved.':'Ready to set up.',downloadedBytes:state==='idle'?0:40,totalBytes:100,canInstall:true};
    window.__TAURI_INTERNALS__.invoke=async(command,args)=>{
      if(command==='get_asr_status')return {state:window.toolsReady?'ready':'not_installed',message:'Local tools'};
      if(command==='get_voice_setup')return structuredClone(window.setupState);
      if(command==='get_setup_plan')return {supported:true,reason:'Tools are matched to this computer.',hardware:{os:'windows',arch:'x86_64',totalRamBytes:64*1024**3,freeRamBytes:32*1024**3,physicalCores:12,nvidia:[]},osVersion:'11',freeStorageBytes:1024**4,routing:{tier:'cpu-high',downloadBytes:100,downloads:[],warnings:[]},features:[]};
      return base(command,args);
    };
  },{state,ready});
  await page.addInitScript(seedDownloadReview,first);
  await page.goto(server.resolvedUrls.local[0]);
  return {page,errors,close:async()=>{await browser.close();await new Promise(resolve=>server.httpServer.close(resolve));}};
}

test('first use opens the highlighted setup first; postponing leaves a prominent Home shortcut without downloading',{skip:!existsSync(executable)},async()=>{
  const fixture=await openFixture({first:true});const {page}=fixture;
  try{
    const intro=page.getByRole('dialog',{name:'Set up your voice tools',exact:true});await intro.waitFor();
    const setup=page.getByRole('region',{name:'Voice tools setup',exact:true});await setup.waitFor();
    assert.ok((await setup.boundingBox()).y<(await page.getByRole('region',{name:'General settings'}).boundingBox()).y);
    assert.deepEqual(await page.evaluate(()=>window.downloadRequests),[]);
    await intro.getByRole('button',{name:'Not now',exact:true}).click();
    const shortcut=page.getByRole('region',{name:'Voice tools setup shortcut'});await shortcut.waitFor();
    assert.equal(await page.locator('.home-page > section').first().getAttribute('aria-label'),'Voice tools setup shortcut');
    await shortcut.getByRole('button',{name:'Set up voice tools'}).click();
    await page.waitForFunction(()=>document.activeElement?.id==='voice-tools-setup');
    assert.deepEqual(await page.evaluate(()=>window.downloadRequests),[]);
    await setup.getByRole('button',{name:'Download voice tools',exact:true}).click();
    const review=page.getByRole('dialog',{name:'Review your download'});await review.getByText('Maximum download',{exact:true}).waitFor();
    assert.deepEqual(await page.evaluate(()=>window.downloadRequests),[]);
    await review.getByRole('button',{name:'Not now',exact:true}).click();
    assert.deepEqual(fixture.errors,[]);
    await page.getByRole('button',{name:'Home',exact:true}).click();
    await shortcut.waitFor();
    await page.screenshot({path:'/tmp/ethiopic-home-setup-shortcut.png'});
  }finally{await fixture.close();}
});

test('paused setup opens directly on restart; Home remains browsable and progress or errors do not hijack navigation',{skip:!existsSync(executable)},async()=>{
  const fixture=await openFixture({state:'paused',ready:true});const {page}=fixture;
  try{
    await page.getByRole('region',{name:'Voice tools setup',exact:true}).waitFor();
    await page.getByRole('button',{name:'Home',exact:true}).click();
    const shortcut=page.getByRole('region',{name:'Voice tools setup shortcut'});
    await shortcut.getByRole('button',{name:'Continue setup'}).waitFor();
    assert.equal(await shortcut.getByRole('progressbar').getAttribute('value'),'40');
    await page.evaluate(()=>window.changeSetup({state:'downloading',stage:'Downloading…',downloadedBytes:55}));
    await page.waitForFunction(()=>document.querySelector('[aria-label="Voice setup download progress"]')?.getAttribute('value')==='55');
    assert.equal(await page.getByRole('button',{name:'Home',exact:true}).getAttribute('aria-current'),'page');
    await page.evaluate(()=>window.changeSetup({state:'failed',error:'Connection interrupted. Your progress is saved.'}));
    await shortcut.getByRole('button',{name:'Retry setup'}).waitFor();
    await page.evaluate(()=>window.changeSetup({state:'completed',ready:true,downloadedBytes:100,error:null}));
    await shortcut.waitFor({state:'hidden'});
    assert.deepEqual(await page.evaluate(()=>window.downloadRequests),[]);
    assert.deepEqual(fixture.errors,[]);
  }finally{await fixture.close();}
});

test('ready installations stay on Home without a setup reminder',{skip:!existsSync(executable)},async()=>{
  const fixture=await openFixture({state:'installed',ready:true});const {page}=fixture;
  try{
    await page.getByRole('heading',{name:'Ethiopic Voice Studio',exact:true}).waitFor();
    await page.waitForFunction(()=>document.querySelector('[aria-label="Voice tools setup shortcut"]')===null);
    assert.equal(await page.getByRole('button',{name:'Home',exact:true}).getAttribute('aria-current'),'page');
    assert.deepEqual(await page.evaluate(()=>window.downloadRequests),[]);
    assert.deepEqual(fixture.errors,[]);
  }finally{await fixture.close();}
});
