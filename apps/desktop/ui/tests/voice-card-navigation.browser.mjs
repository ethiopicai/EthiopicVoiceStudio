import test from 'node:test';
import assert from 'node:assert/strict';
import {existsSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
import {preview} from 'vite';
import {chromium} from 'playwright-core';
import {seedVoiceHistory} from './voice-history-fixture.mjs';
const executable=process.env.CHROME_PATH||'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';

test('voice History indexes the main card list and jumps without scrolling History or eagerly opening other audio',{skip:!existsSync(executable),timeout:60000},async()=>{
 const server=await preview({root:fileURLToPath(new URL('..',import.meta.url)),preview:{host:'127.0.0.1',port:0,open:false}});
 const browser=await chromium.launch({executablePath:executable,headless:true});
 try{
  for(const [view,historyName,label,id] of [
   ['Voice Cleanup','Cleanup history','Open original recording: Recording 0.wav','voice-card-cleanup-recording-extra-0'],
   ['Voice Editing','Editing history','Open editing recording: Recording 11.wav','voice-card-editing-extra-11'],
   ['Text to Speech','Voice history','Open generated speech: Speech 11.wav','voice-card-voice-project-extra-11']
  ]){
   const page=await browser.newPage({viewport:{width:1400,height:850},colorScheme:'dark'}),errors=[];
   page.on('pageerror',error=>errors.push(error.message));await page.addInitScript(seedVoiceHistory,12);
   await page.goto(server.resolvedUrls.local[0]);await page.locator(`button.nav-item[aria-label="${view}"]`).click();
   const history=page.getByRole('region',{name:historyName,exact:true}),entry=history.getByRole('button',{name:label,exact:true});
   await entry.waitFor();const cards=page.locator(view==='Text to Speech'?'.generated-library [data-workspace-card]':'[data-workspace-card]'),count=await history.locator('.history-entry').count();
   assert.equal(await cards.count(),count,`${view}: every history row has a main card`);
   assert.equal(await page.evaluate(()=>window.voiceCalls.filter(command=>['get_cleanup_audio','get_cleanup_recording_audio','get_voice_edit_audio'].includes(command)).length),0,`${view}: metadata listing does not load recordings`);
   await entry.scrollIntoViewIfNeeded();await entry.focus();
   const before=await page.evaluate(()=>({history:document.querySelector('.history-scroll').scrollTop,window:window.scrollY}));
   await page.keyboard.press('Enter');const card=page.locator(`[data-workspace-card="${id}"]`);
   await page.waitForFunction(id=>document.getElementById(id)?.querySelector('.card-heading').getAttribute('aria-expanded')==='true',id);
   // Media fetching and Svelte's final render finish before the scroll helper runs.
   await page.waitForFunction(()=>document.querySelector('[data-card-scroller]').scrollTop>0);
   const after=await card.evaluate(element=>{const scroller=element.closest('[data-card-scroller]'),a=element.getBoundingClientRect(),b=scroller.getBoundingClientRect();return {top:a.top-b.top,visible:a.bottom>b.top&&a.top<b.bottom,scroll:scroller.scrollTop,maximum:scroller.scrollHeight-scroller.clientHeight,history:document.querySelector('.history-scroll').scrollTop,window:window.scrollY};});
   assert.equal(after.visible,true,`${view}: selected card is visible`);assert.ok(after.top>=-1&&(after.top<120||Math.abs(after.scroll-after.maximum)<2),`${view}: jump lands at header or end of list: ${JSON.stringify(after)}`);
   assert.equal(after.history,before.history,`${view}: History stays at its browsing position`);assert.equal(after.window,before.window,`${view}: app window does not scroll`);
   assert.equal(await entry.evaluate(element=>document.activeElement===element),true,'keyboard focus stays in History');
   assert.equal(await cards.count(),count,`${view}: opening a card never replaces the main list`);
   const reads=await page.evaluate(()=>window.voiceCalls.filter(command=>['get_cleanup_audio','get_cleanup_recording_audio','get_voice_edit_audio'].includes(command)).length);
   assert.equal(reads,id.includes('voice-profile')?0:1,`${view}: only the selected native recording is loaded`);
   if(view==='Voice Editing')await page.getByRole('textbox',{name:'Recording transcript',exact:true}).fill('Unsaved correction');
   await entry.click();
   assert.equal(await page.evaluate(()=>window.voiceCalls.filter(command=>['get_cleanup_audio','get_cleanup_recording_audio','get_voice_edit_audio'].includes(command)).length),reads,'clicking the active History entry reuses its audio');
   if(view==='Voice Editing')assert.equal(await page.getByRole('textbox',{name:'Recording transcript',exact:true}).inputValue(),'Unsaved correction');
   const cardCheckbox=card.getByRole('checkbox',{name:/^Select card:/});await cardCheckbox.check();assert.equal(await history.locator('input:checked').count(),1,'card selection also checks its History row');
   assert.equal(await card.locator('.card-heading').getAttribute('aria-expanded'),'true','selection does not collapse a card');
   await card.locator('.card-heading').click();assert.equal(await card.locator('.card-heading').getAttribute('aria-expanded'),'false');
   await entry.click();await page.waitForFunction(id=>document.getElementById(id)?.querySelector('.card-heading').getAttribute('aria-expanded')==='true',id);
   assert.equal(await cards.count(),count,'History reopens a collapsed card in the same list');
   await page.screenshot({path:`/tmp/ethiopic-card-navigation-${view.toLowerCase().replaceAll(' ','-')}.png`});
   await page.getByRole('button',{name:`Hide ${historyName.toLowerCase()}`,exact:true}).click();
   assert.equal(await cards.count(),count,'collapsing History preserves the main cards');
   assert.deepEqual(errors,[]);await page.close();
  }
 }finally{await browser.close();await new Promise((resolve,reject)=>server.httpServer.close(error=>error?reject(error):resolve()));}
});
