import test from 'node:test';
import {seedVoiceHistory} from './voice-history-fixture.mjs';
import assert from 'node:assert/strict';
import {existsSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
import {preview} from 'vite';
import {chromium} from 'playwright-core';
const executable=process.env.CHROME_PATH||'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';

test('all voice tools share card/history selection, confirmed deletion, partial failure retry, and protection for presets and unselected work',{skip:!existsSync(executable),timeout:60000},async()=>{
 const server=await preview({root:fileURLToPath(new URL('..',import.meta.url)),preview:{host:'127.0.0.1',port:0,open:false}});
 const browser=await chromium.launch({executablePath:executable,headless:true});
 try{
  for(const view of ['Voice Cleanup','Voice Editing','Text to Speech']){
   const page=await browser.newPage({viewport:{width:1400,height:1100},colorScheme:'dark'}),errors=[];page.on('pageerror',error=>errors.push(error.message));
   await page.addInitScript(seedVoiceHistory,view==='Text to Speech'?2:0);
   const settings=view==='Voice Cleanup'?{history:'Cleanup history',first:'Select original recording: Original one.wav',second:'Select cleaned recording: Cleaned one.wav',open:'Open original recording: Original one.wav',current:'Select current cleanup recording',fail:'result-one'}:view==='Voice Editing'?{history:'Editing history',first:'Select editing recording: Edit one.wav',second:'Select editing recording: Edit two.wav',open:'Open editing recording: Edit one.wav',current:'Select current editing recording',fail:'edit-two'}:{history:'Voice history',first:'Select generated speech: Generated one.wav',second:'Select generated speech: Speech 0.wav',open:'Open generated speech: Generated one.wav',current:'Select current generated speech',fail:'extra-0'};
   await page.goto(server.resolvedUrls.local[0]);await page.locator(`button.nav-item[aria-label="${view}"]`).click();
   const history=page.getByRole('region',{name:settings.history,exact:true}),first=history.getByRole('checkbox',{name:settings.first,exact:true}),second=history.getByRole('checkbox',{name:settings.second,exact:true});
   await first.waitFor();const audioReads=await page.evaluate(()=>window.voiceCalls.filter(command=>command==='get_cleanup_audio'||command==='get_cleanup_recording_audio'||command==='get_voice_edit_audio').length);
   await first.check();await second.check();
   assert.equal(await page.evaluate(()=>window.voiceCalls.filter(command=>command==='get_cleanup_audio'||command==='get_cleanup_recording_audio'||command==='get_voice_edit_audio').length),audioReads,`${view}: selecting must not open a recording`);
   const all=history.getByRole('checkbox',{name:'Select all recordings',exact:true});assert.equal(await all.evaluate(input=>input.indeterminate),true);
   await history.getByRole('button',{name:settings.open,exact:true}).click();
   const current=page.getByRole('checkbox',{name:settings.current,exact:true});await current.waitFor();assert.equal(await current.isChecked(),true,`${view}: card and history selection agree`);
   await page.screenshot({path:`/tmp/ethiopic-selection-${view.toLowerCase().replaceAll(' ','-')}.png`});
   if(view==='Text to Speech')await page.evaluate(()=>window.referenceBeforeDelete=document.querySelector('.generated-card-body .waveform-player audio'));
   const remove=history.getByRole('button',{name:'Delete selected',exact:true});
   await remove.click();let dialog=page.getByRole('dialog').filter({visible:true});await dialog.getByRole('heading',{name:'Delete 2 recordings?',exact:true}).waitFor();await dialog.getByRole('button',{name:'Keep it',exact:true}).click();
   assert.equal(await page.evaluate(()=>window.deleteRequests.length),0);
   if(view==='Voice Editing'){
    await page.evaluate(()=>window.voiceBusy=true);await page.waitForFunction(()=>document.querySelector('[data-editing-id]')?.disabled===true);assert.equal(await remove.isDisabled(),true);assert.equal(await all.isDisabled(),true);await page.evaluate(()=>window.voiceBusy=false);await page.waitForFunction(()=>document.querySelector('[data-editing-id]')?.disabled===false);
   }
   await page.evaluate(()=>window.failRequest=true);await remove.click();dialog=page.getByRole('dialog').filter({visible:true});await dialog.getByRole('button',{name:'Delete from app',exact:true}).click();await dialog.getByRole('alert').waitFor();assert.equal(await first.isChecked(),true);await dialog.getByRole('button',{name:'Keep it',exact:true}).click();
   await page.evaluate(id=>{window.failRequest=false;window.failId=id;},settings.fail);await remove.click();dialog=page.getByRole('dialog').filter({visible:true});await dialog.getByRole('button',{name:'Delete from app',exact:true}).click();await first.waitFor({state:'detached'});await dialog.getByRole('alert').filter({hasText:'remaining recordings are still selected'}).waitFor();assert.equal(await second.isChecked(),true);assert.equal(await current.count(),0,`${view}: deleting the open recording clears its card and playback`);
   await dialog.getByRole('button',{name:'Keep it',exact:true}).click();await page.evaluate(()=>window.failId='');await remove.click();dialog=page.getByRole('dialog').filter({visible:true});await dialog.getByRole('heading',{name:'Delete recording?',exact:true}).waitFor();await dialog.getByRole('button',{name:'Delete from app',exact:true}).click();await second.waitFor({state:'detached'});
   if(view==='Text to Speech')assert.equal(await page.evaluate(()=>window.referenceBeforeDelete.paused&&window.referenceBeforeDelete.getAttribute('src')===null),true,'deleted generated playback is stopped and detached');
   assert.equal(await history.locator('.history-entry').count(),1,`${view}: an unselected item remains`);
   await all.check();await history.getByRole('button',{name:'Clear',exact:true}).click();assert.equal(await remove.count(),0);
   await all.check();await remove.click();dialog=page.getByRole('dialog').filter({visible:true});await dialog.getByRole('button',{name:'Delete from app',exact:true}).click();await page.waitForFunction(()=>document.querySelectorAll('.history-entry').length===0);assert.equal(await all.isDisabled(),true);
   await page.waitForFunction(()=>document.activeElement!==document.body&&!document.activeElement.closest('dialog'));
   if(view==='Text to Speech'){const choice=page.getByRole('combobox',{name:'Choose a voice',exact:true});assert.equal(await choice.locator('option[value="preset:default"]').count(),1);assert.equal(await choice.locator('option[value="preset:demo"]').count(),0);assert.equal(await choice.locator('option[value^="profile:"]').count(),4);}
   assert.equal(await page.evaluate(()=>window.deleteRequests.flat().some(item=>'name' in item)),false);
   assert.deepEqual(errors,[]);await page.close();
  }
 }finally{await browser.close();await new Promise((resolve,reject)=>server.httpServer.close(error=>error?reject(error):resolve()));}
});
