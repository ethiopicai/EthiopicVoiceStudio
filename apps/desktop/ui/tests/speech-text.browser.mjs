import test from 'node:test';
import assert from 'node:assert/strict';
import {existsSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
import {preview} from 'vite';
import {chromium} from 'playwright-core';
import {seedVoiceHistory} from './voice-history-fixture.mjs';
const executable=process.env.CHROME_PATH||'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
test('synthesis and regeneration always use checked spoken text, keep original input, and refuse ambiguous numbers before model work',{skip:!existsSync(executable),timeout:60000},async()=>{
 const server=await preview({root:fileURLToPath(new URL('..',import.meta.url)),preview:{host:'127.0.0.1',port:0,open:false}}),browser=await chromium.launch({executablePath:executable,headless:true});
 try{
  const page=await browser.newPage({viewport:{width:1440,height:1100}}),errors=[];page.on('pageerror',error=>errors.push(error.message));await page.addInitScript(seedVoiceHistory,0);
  await page.addInitScript(()=>{if(window.self!==window.top)return;const base=window.__TAURI_INTERNALS__.invoke;window.textRequests=[];window.textVersions=[];window.__TAURI_INTERNALS__.invoke=async(command,args={})=>{
   if(command==='get_voice_generation_status')return {ready:true,consent:true,busy:false,provider:'tts-flow_lm-90m-amh-fp32',requiresReferenceText:false,defaultVoice:true};
   if(command==='get_default_voice')return {name:'Default.wav',bytes:window.voiceBytes};
   if(command==='synthesize_speech'){const input=args.input;window.textRequests.push(structuredClone(input));const projectId=`text-${window.textRequests.length}`,groupId=input.groupId??projectId;window.textVersions.push({projectId,text:input.text,originalText:input.originalText,reference:{voice:input.voice,name:input.name,text:input.referenceText}});window.voiceGenerations=window.voiceGenerations.filter(item=>item.id!==groupId).concat({id:groupId,name:'Checked speech.wav',text:input.text,revision:0,created:1,currentVersion:projectId,versionCount:window.textVersions.length});return {groupId,projectId};}
   if(command==='get_speech_versions'&&args.groupId==='text-1')return structuredClone(window.textVersions);
   return base(command,args);
  };});
  await page.goto(server.resolvedUrls.local[0]);await page.locator('button.nav-item[aria-label="Text to Speech"]').click();
  const script=page.getByLabel('Script',{exact:true});await script.fill('ዶ/ር አበበ እና ኢ/ር ሐና 12 ሰዎች።');const preview=page.getByRole('region',{name:'Speech text check'}),processed=preview.locator('.prepared-script'),toggle=preview.locator('summary');
  await toggle.waitFor();assert.equal(await processed.isVisible(),false,'processed wording is optional and hidden by default');await toggle.click();await processed.waitFor();assert.equal(await processed.textContent(),'ዶክተር አበበ እና ኢንጂነር ሀና አስራ ሁለት ሰዎች።');assert.equal(await script.inputValue(),'ዶ/ር አበበ እና ኢ/ር ሐና 12 ሰዎች።');await toggle.click();assert.equal(await processed.isVisible(),false);
  await page.getByRole('button',{name:'Generate speech',exact:true}).click();const card=page.locator('#voice-card-voice-project-text-1');await card.getByRole('button',{name:'Edit & regenerate',exact:true}).waitFor();
  let request=await page.evaluate(()=>window.textRequests[0]);assert.equal(request.text,'ዶክተር አበበ እና ኢንጂነር ሀና አስራ ሁለት ሰዎች።');assert.equal(request.originalText,'ዶ/ር አበበ እና ኢ/ር ሐና 12 ሰዎች።');
  await card.getByRole('button',{name:'Edit & regenerate',exact:true}).click();assert.equal(await script.inputValue(),'ዶ/ር አበበ እና ኢ/ር ሐና 12 ሰዎች።');await script.fill('ዋጋ 25%');await page.getByRole('button',{name:'Manage voices',exact:true}).click();await page.getByLabel('Reference transcript (optional)',{exact:true}).fill('ዶ/ር ጤና 12');await page.getByRole('button',{name:'Use this voice in script',exact:true}).click();await card.getByRole('button',{name:'Regenerate speech',exact:true}).click();await page.waitForFunction(()=>window.textRequests.length===2);
  request=await page.evaluate(()=>window.textRequests[1]);assert.equal(request.text,'ዋጋ ሀያ አምስት በመቶ');assert.equal(request.originalText,'ዋጋ 25%');assert.equal(request.referenceText,'ዶ/ር ጤና 12','the exact reference transcript is preserved');assert.equal(request.groupId,'text-1');
  await script.fill('ኡ/ኡ አበበ');await preview.getByRole('alert').filter({hasText:'Write unfamiliar'}).waitFor();assert.equal(await card.getByRole('button',{name:'Regenerate speech',exact:true}).isDisabled(),true);assert.equal(await page.evaluate(()=>window.textRequests.length),2);
  await script.fill('ዶ.ር አበበ');await toggle.waitFor();assert.equal(await processed.isVisible(),false,'text validation must not automatically reveal the preview');await toggle.click();await preview.getByText('ዶክተር አበበ',{exact:true}).waitFor();await preview.getByText(/Abbreviations expanded/).waitFor();await script.fill('ኢ/ር ሀና');await preview.getByText('ኢንጂነር ሀና',{exact:true}).waitFor();await script.fill('ሰላም');await preview.getByText('ሰላም',{exact:true}).waitFor();assert.equal(await preview.getByRole('button',{name:'Use prepared text',exact:true}).count(),0,'unchanged text can still be inspected without a redundant apply action');await toggle.click();
  await script.fill('12/03/2026');await preview.getByRole('alert').filter({hasText:'Write dates'}).waitFor();assert.equal(await card.getByRole('button',{name:'Regenerate speech',exact:true}).isDisabled(),true);assert.equal(await page.evaluate(()=>window.textRequests.length),2);
  await script.fill('0 '.repeat(6000));assert.equal(await card.getByRole('button',{name:'Regenerate speech',exact:true}).isDisabled(),true);await preview.getByText(/12,000 characters/).waitFor();
  assert.deepEqual(errors,[]);
 }finally{await browser.close();await new Promise((resolve,reject)=>server.httpServer.close(error=>error?reject(error):resolve()));}
});
