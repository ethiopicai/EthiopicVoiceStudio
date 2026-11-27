import test from 'node:test';
import assert from 'node:assert/strict';
import {existsSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
import {preview} from 'vite';
import {chromium} from 'playwright-core';
import {seedVoiceHistory} from './voice-history-fixture.mjs';
const executable=process.env.CHROME_PATH||'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
test('speech history and reference voices have separate lists, selection, refresh and deletion controls',{skip:!existsSync(executable),timeout:60000},async()=>{
 const server=await preview({root:fileURLToPath(new URL('..',import.meta.url)),preview:{host:'127.0.0.1',port:0,open:false}}),browser=await chromium.launch({executablePath:executable,headless:true});
 try{
  for(const remove of ['profiles','speech']){
   const page=await browser.newPage({viewport:{width:1440,height:1100},colorScheme:'dark'}),errors=[];page.on('pageerror',error=>errors.push(error.message));await page.addInitScript(seedVoiceHistory,0);await page.goto(server.resolvedUrls.local[0]);await page.locator('button.nav-item[aria-label="Text to Speech"]').click();
   const history=page.getByRole('region',{name:'Voice history',exact:true}),voices=page.getByRole('region',{name:'Reference voices',exact:true});
   await history.getByRole('button',{name:'Open generated speech: Generated one.wav',exact:true}).waitFor();assert.equal(await history.locator('.history-entry').count(),1);assert.equal(await history.getByText('Saved voice profile',{exact:true}).count(),0);assert.equal(await history.locator('[data-profile-id]').count(),0);
   await voices.locator('#voice-library > summary').click();const profiles=voices.locator('[data-workspace-card]');await profiles.first().waitFor();assert.equal(await profiles.count(),2);
   const allSpeech=history.getByRole('checkbox',{name:'Select all recordings',exact:true}),allProfiles=voices.getByRole('checkbox',{name:'Select all voice profiles',exact:true});await allSpeech.check();assert.equal(await allProfiles.isChecked(),false);await allProfiles.check();assert.equal(await allSpeech.isChecked(),true);
   await history.getByRole('button',{name:'Clear',exact:true}).click();assert.equal(await allProfiles.isChecked(),true,'clearing speech selection leaves profile selection alone');
   if(remove==='profiles'){
    await page.screenshot({path:'/tmp/ethiopic-speech-reference-separated.png'});
    await voices.getByRole('button',{name:'Open card: Voice one',exact:true}).click();await page.getByRole('region',{name:'Reference recording',exact:true}).waitFor();
    await page.getByLabel('Script',{exact:true}).fill('Unfinished new speech');await history.getByRole('button',{name:'Open generated speech: Generated one.wav',exact:true}).click();await page.getByRole('button',{name:'Edit & regenerate',exact:true}).click();await page.getByRole('button',{name:'Manage voices',exact:true}).click();
    await voices.getByRole('button',{name:'Delete selected profiles',exact:true}).click();let dialog=page.getByRole('dialog').filter({visible:true});await dialog.getByRole('heading',{name:'Delete 2 voice profiles?',exact:true}).waitFor();await dialog.getByRole('button',{name:'Keep it',exact:true}).click();assert.equal(await profiles.count(),2);
    await voices.getByRole('button',{name:'Delete selected profiles',exact:true}).click();dialog=page.getByRole('dialog').filter({visible:true});await dialog.getByRole('button',{name:'Delete from app',exact:true}).click();await profiles.first().waitFor({state:'detached'});
    assert.equal(await history.locator('.history-entry').count(),1);assert.equal(await page.evaluate(()=>window.voiceGenerations.length),1);assert.equal(await page.evaluate(()=>window.deleteRequests.length),0,'profile deletion never sends a generated-audio deletion request');await page.getByRole('button',{name:'Done editing',exact:true}).click();assert.equal(await page.getByLabel('Choose a voice',{exact:true}).inputValue(),'preset:default');assert.equal(await page.getByLabel('Script',{exact:true}).inputValue(),'Unfinished new speech','deleting the draft reference preserves text and restores a usable default voice');
    await history.getByRole('button',{name:'Open generated speech: Generated one.wav',exact:true}).click();await page.locator('.generated-library .waveform-player').waitFor();
   }else{
    await allSpeech.check();await history.getByRole('button',{name:'Delete selected',exact:true}).click();await page.getByRole('dialog').filter({visible:true}).getByRole('button',{name:'Delete from app',exact:true}).click();await history.locator('.history-entry').waitFor({state:'detached'});assert.equal(await profiles.count(),2);assert.equal(await allProfiles.isChecked(),true);assert.deepEqual(await page.evaluate(()=>window.deleteRequests[0]),[{kind:'voice-project',id:'generation-one',revision:0}]);
    await voices.getByRole('button',{name:'Refresh voices',exact:true}).click();await profiles.first().waitFor();assert.equal(await profiles.count(),2);await history.getByText('No generated speech yet.',{exact:true}).waitFor();
   }
   await page.screenshot({path:`/tmp/ethiopic-reference-panel-${remove}.png`});assert.deepEqual(errors,[]);await page.close();
  }
 }finally{await browser.close();await new Promise((resolve,reject)=>server.httpServer.close(error=>error?reject(error):resolve()));}
});
