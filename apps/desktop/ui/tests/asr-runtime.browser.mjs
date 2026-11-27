import test from 'node:test';
import assert from 'node:assert/strict';
import {existsSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
import {preview} from 'vite';
import {chromium} from 'playwright-core';
const executable=process.env.CHROME_PATH||'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
test('local-model polling stays quiet, manual checks coalesce, and retention settings persist through navigation and failures',{skip:!existsSync(executable)},async()=>{
 const server=await preview({root:fileURLToPath(new URL('..',import.meta.url)),preview:{host:'127.0.0.1',port:0,open:false}});
 const browser=await chromium.launch({executablePath:executable,headless:true});
 try{
  const page=await browser.newPage();
  await page.addInitScript(()=>{
   window.runtimeChecks=0;window.holdRuntime=false;window.runtime={keepReady:true,state:'waiting'};window.failSave=false;window.failUnload=false;window.unloadCalls=0;
   window.__TAURI_INTERNALS__={invoke:async(command,args)=>{
    if(command==='get_asr_runtime_status'){window.runtimeChecks++;if(window.holdRuntime)return new Promise(resolve=>window.finishRuntimeCheck=()=>{window.finishRuntimeCheck=null;resolve({...window.runtime});});return {...window.runtime};}
    if(command==='unload_asr_model'){window.unloadCalls++;if(window.failUnload)throw 'Finish or stop transcription or dictation before unloading its model.';window.runtime={...window.runtime,state:'unloaded'};return {...window.runtime};}
    if(command==='set_asr_keep_ready'){
     if(window.failSave)throw Error('Storage unavailable');
     window.runtime={keepReady:args.keepReady,state:args.keepReady?'waiting':'unloaded'};return {...window.runtime};
    }
    if(command==='get_asr_status'||command==='get_cleanup_status')return {state:'ready',message:'Ready',modelVersion:'fixture'};
    if(command.startsWith('get_active_'))return null;
    if(command==='get_fixture_progress')return {state:'idle',downloadedBytes:0,totalBytes:0};
    throw Error('Not required by runtime settings fixture');
   }};
  });
  await page.goto(server.resolvedUrls.local[0]);await page.getByRole('button',{name:'Settings',exact:true}).click();
  const checkbox=page.getByRole('checkbox',{name:'Keep voice tools ready',exact:true});
  await checkbox.waitFor();await page.waitForFunction(()=>!document.querySelector('.keep-ready input').disabled);
  assert.equal(await checkbox.isChecked(),true);await page.locator('.keep-ready').getByText('Loaded · Waiting for recordings',{exact:true}).waitFor();
  const local=page.getByRole('region',{name:'Local models',exact:true});
  const check=local.getByRole('button',{name:'Check again',exact:true});
  const width=(await check.boundingBox()).width;
  const idleChecks=await page.evaluate(()=>window.runtimeChecks);await page.waitForTimeout(2800);assert.equal(await page.evaluate(()=>window.runtimeChecks),idleChecks,'idle models are not polled every few seconds');
  await page.evaluate(()=>{window.holdRuntime=true;window.dispatchEvent(new Event('studio-tools-changed'));});
  await page.waitForFunction(()=>!!window.finishRuntimeCheck);
  assert.equal(await check.isEnabled(),true,'background polling must not flash or disable the manual button');
  assert.equal(await check.innerText(),'Check again');
  const checks=await page.evaluate(()=>window.runtimeChecks);
  await check.click();const checking=local.getByRole('button',{name:'Checking…',exact:true});
  await checking.waitFor();assert.equal(await checking.isDisabled(),true);assert.equal(await checking.getAttribute('aria-busy'),'true');
  assert.equal((await checking.boundingBox()).width,width,'manual progress does not resize the button');
  assert.equal(await page.evaluate(()=>window.runtimeChecks),checks,'manual check joins the pending poll');
  await page.evaluate(()=>{window.holdRuntime=false;window.finishRuntimeCheck();});
  await check.waitFor();assert.equal(await check.isEnabled(),true);
  const unload=page.getByRole('button',{name:'Unload from memory',exact:true});await unload.click();
  await page.waitForFunction(()=>window.runtime.state==='unloaded');assert.equal(await checkbox.isChecked(),true,'unloading once preserves retention for the next job');assert.equal(await unload.isDisabled(),true);
  await page.evaluate(()=>{window.runtime.state='running';});await page.locator('.local-models').getByRole('button',{name:'Check again',exact:true}).click();assert.equal(await unload.isDisabled(),true,'active recognition cannot be unloaded');assert.equal(await page.evaluate(()=>window.unloadCalls),1);
  await page.evaluate(()=>{window.runtime.state='waiting';window.failUnload=true;});await page.locator('.local-models').getByRole('button',{name:'Check again',exact:true}).click();await unload.click();await page.getByRole('alert').filter({hasText:'Finish or stop transcription'}).waitFor();assert.equal(await checkbox.isChecked(),true);
  await page.evaluate(()=>window.failUnload=false);

  await page.getByRole('button',{name:'Home',exact:true}).click();await page.getByRole('button',{name:'Settings',exact:true}).click();
  await page.waitForFunction(()=>document.querySelector('.keep-ready input')?.checked===true);
  await page.evaluate(()=>window.failSave=true);await checkbox.click();
  await page.getByRole('alert').filter({hasText:'Could not save this preference'}).waitFor();
  assert.equal(await checkbox.isChecked(),true);
  await page.evaluate(()=>window.failSave=false);await checkbox.uncheck();
  await page.waitForFunction(()=>window.runtime.keepReady===false);
  await page.locator('.keep-ready').getByText('Loaded · Waiting for recordings',{exact:true}).waitFor({state:'detached'});
 }finally{await browser.close();await new Promise(r=>server.httpServer.close(r));}
});
