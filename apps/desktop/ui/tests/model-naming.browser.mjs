import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import {fileURLToPath} from 'node:url';
import {preview} from 'vite';
import {chromium} from 'playwright-core';
test('model details group new releases by family and persist an aliased saved selection',async()=>{
 const manifest=JSON.parse(await readFile(new URL('../../resources/studio-manifest.json',import.meta.url),'utf8'));
 const [old,current]=Object.entries(manifest.aliases).find(([,id])=>id.startsWith('tts-flow_lm'));
 const server=await preview({root:fileURLToPath(new URL('..',import.meta.url)),preview:{host:'127.0.0.1',port:0,open:false}});
 const browser=await chromium.launch({executablePath:process.env.CHROME_PATH||'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',headless:true});
 try{
  const page=await browser.newPage();
  await page.addInitScript(({manifest,old})=>{
   localStorage.setItem('ethiopic-studio.model-details',old);
   window.__TAURI_INTERNALS__={invoke:async command=>{
    if(command==='get_studio_model_source')return {manifest};
    if(command==='get_voice_generation_status')return {provider:'tts-omnivoice-fp16',modelVersion:'v1.0',runtimePresent:true,ready:true,loaded:true,reason:'Ready'};
    if(command==='get_voice_edit_status')return {selection:{tier:'gpu-modern',tts:'tts-omnivoice-fp16',reason:'Ready'},runtimePresent:true,ready:true,loaded:true};
    if(command==='get_asr_status')return {state:'ready',message:'Ready',modelVersion:'v1.0',release:manifest.variants[0].release};
    if(command==='get_cleanup_status')return {state:'not_installed',message:'Not installed'};
    if(command==='get_asr_runtime_status')return {keepReady:true,state:'unloaded'};
    if(command.startsWith('get_active_'))return null;
    throw Error('Outside this fixture');
   }};
  },{manifest,old});
  await page.goto(server.resolvedUrls.local[0]);await page.getByRole('button',{name:'Settings',exact:true}).click();
  await page.getByText('Browse model details',{exact:true}).click();
  const picker=page.getByLabel('Model release',{exact:true});await picker.waitFor();
  assert.equal(await picker.inputValue(),current);
  assert.equal(await page.evaluate(()=>localStorage.getItem('ethiopic-studio.model-details')),current);
  assert.equal(await picker.locator('optgroup').count(),new Set(manifest.variants.map(v=>v.family)).size);
  for(const variant of manifest.variants)assert.ok((await picker.locator(`option[value="${variant.id}"]`).innerText()).includes(variant.release));
  const text=await page.getByRole('region',{name:'Local models',exact:true}).innerText();
  for(const oldId of Object.keys(manifest.aliases))assert.ok(!text.includes(oldId));
  const licenses=page.getByRole('region',{name:'Models and licenses',exact:true});
  await licenses.getByText('Voice Editing · Local runtime and models',{exact:true}).click();
  const editor=licenses.locator('details').filter({has:page.locator('summary',{hasText:'Voice Editing · Local runtime and models'})});
  await editor.getByText(manifest.variants.find(v=>v.id==='tts-codec_llm-0.6b-amh-fp16').release,{exact:true}).waitFor();
  assert.ok(!(await editor.innerText()).includes('tts-omnivoice'));
  const weights=licenses.getByLabel('Model weight licenses',{exact:true});
  for(const label of ['Amharic speech-generation model','Amharic ASR and word alignment','Amharic text corrector']){
    const row=weights.locator('div').filter({has:page.getByText(label,{exact:true})});
    assert.match(await row.innerText(),/CC BY 4.0/);assert.match(await row.innerText(),/commercial use with attribution/);
  }
  await licenses.getByText('Upstream software & attribution',{exact:true}).click();
  const software=licenses.getByLabel('Software licenses',{exact:true});
  for(const label of ['Speech-generation library · OmniVoice','Upstream ASR · Meta Omnilingual ASR'])assert.match(await software.locator('div').filter({has:page.getByText(label,{exact:true})}).innerText(),/Apache-2.0/);
 }finally{await browser.close();await new Promise(resolve=>server.httpServer.close(resolve));}
});
