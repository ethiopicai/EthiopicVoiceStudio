import test from 'node:test';
import assert from 'node:assert/strict';
import {existsSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
import {preview} from 'vite';
import {chromium} from 'playwright-core';
import {seedDictation} from './dictation-fixture.mjs';
const executable=process.env.CHROME_PATH||'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';

test('dictation conversation preserves recording cards, folds live text into saved text, and releases microphone',{skip:!existsSync(executable)},async()=>{
  const server=await preview({root:fileURLToPath(new URL('..',import.meta.url)),preview:{host:'127.0.0.1',port:0,open:false}});
  const browser=await chromium.launch({executablePath:executable,headless:true,args:['--use-fake-device-for-media-stream','--use-fake-ui-for-media-stream']});
  try{
    const page=await browser.newPage({viewport:{width:1100,height:780}});
    await page.addInitScript(seedDictation,0);
    await page.goto(server.resolvedUrls.local[0]);
    await page.locator('button.nav-item[aria-label="Dictation"]').click();
    assert.equal(await page.locator('fieldset').count(),0);
    assert.equal(await page.getByText('Speak to write',{exact:true}).count(),0);
    await page.getByRole('button',{name:'Start recording',exact:true}).click();
    await page.getByRole('button',{name:'Stop recording',exact:true}).waitFor();
    await page.locator('.partial').filter({hasText:'first-tail'}).waitFor();
    assert.equal(await page.evaluate(()=>window.calls.indexOf('start_dictation')<window.calls.lastIndexOf('microphone')),true);
    await page.evaluate(()=>window.tail='replacement-tail');
    await page.locator('.partial').filter({hasText:'replacement-tail'}).waitFor();
    await page.getByRole('button',{name:'Stop recording',exact:true}).click();
    const first=page.locator('[data-transcript-id="live-1"]');await first.waitFor();
    assert.match(await first.textContent(),/last-word-live-1/);
    assert.equal(await page.getByRole('region',{name:'Live transcript',exact:true}).count(),0);
    assert.equal(await page.evaluate(()=>window.microphoneTracks.every(t=>t.readyState==='ended')),true);
    await first.getByRole('button',{name:'Copy text',exact:true}).click();
    assert.equal(await page.evaluate(()=>window.clipboard),'fixed last-word-live-1');
    await first.getByRole('button',{name:'Copy WAV path for recording 1',exact:true}).click();
    assert.equal(await page.evaluate(()=>window.clipboard),'/mock/live-1.wav');
    for(const label of ['WebVTT (.vtt)','SubRip (.srt)','All word data (.json)']){
      await first.locator('.timing-export summary').click();await first.getByRole('button',{name:label,exact:true}).click();
    }
    assert.deepEqual(await page.evaluate(()=>window.exports.map(item=>item.format)),['vtt','srt','json']);
    const highlight=first.getByRole('checkbox',{name:'Highlight words during playback',exact:true});
    assert.equal(await highlight.isChecked(),true);
    await first.getByRole('button',{name:'Play Recording 1',exact:true}).click();
    await page.waitForFunction(()=>!!document.querySelector('#transcript-live-1 .playback-word'));
    await first.getByRole('button',{name:'Pause Recording 1',exact:true}).click();
    await page.waitForFunction(()=>!document.querySelector('#transcript-live-1 .playback-word'));

    await first.locator('[data-word-id="0"]').click();
    await first.getByLabel('Correct selected word',{exact:true}).fill('corrected');
    await first.getByRole('button',{name:'Save correction — I checked this word',exact:true}).click();
    await first.getByRole('button',{name:'Copy text',exact:true}).click();
    assert.equal(await page.evaluate(()=>window.clipboard),'corrected last-word-live-1');


    await page.getByRole('button',{name:'Start recording',exact:true}).click();
    await page.getByRole('button',{name:'Stop recording',exact:true}).waitFor();
    await page.waitForTimeout(650);
    assert.equal(await first.count(),1);
    await page.getByRole('button',{name:'Stop recording',exact:true}).click();
    await page.locator('[data-transcript-id="live-2"]').waitFor();
    assert.equal(await page.locator('[data-transcript-id]').count(),2);
    await page.locator('button.nav-item[aria-label="Home"]').click();
    await page.locator('button.nav-item[aria-label="Dictation"]').click();
    await page.locator('[data-transcript-id="live-1"]').waitFor();
    assert.equal(await page.locator('[data-transcript-id]').count(),2);
    const divider=page.getByRole('separator',{name:'Resize transcription history'});
    const previousWidth=Number(await divider.getAttribute('aria-valuenow'));await divider.focus();await page.keyboard.press('ArrowRight');
    assert.equal(Number(await divider.getAttribute('aria-valuenow')),previousWidth+20);
    const dictationCard=await page.locator('[data-transcript-id="live-2"]').boundingBox();
    await page.locator('button.nav-item[aria-label="Transcription"]').click();
    const sameCard=page.locator('[data-transcript-id="live-2"]');await sameCard.waitFor();
    assert.equal(await sameCard.locator('.text-actions').count(),1);
    assert.equal(await sameCard.getByRole('checkbox',{name:'Highlight words during playback',exact:true}).isChecked(),true);
    assert.equal(await sameCard.getByRole('button',{name:'Copy text',exact:true}).count(),1);
    const transcriptionCard=await sameCard.boundingBox();
    assert.equal(transcriptionCard.width,dictationCard.width);
    assert.equal(await page.getByRole('button',{name:'Import recording',exact:true}).count(),1);
    assert.equal(await page.getByRole('button',{name:'Start recording',exact:true}).count(),0);
    await page.screenshot({path:'/tmp/ethiopic-transcription-unified.png'});
    await page.locator('button.nav-item[aria-label="Dictation"]').click();
    await page.locator('[data-transcript-id="live-2"]').waitFor();
    await page.screenshot({path:'/tmp/ethiopic-dictation-conversation.png'});
    const second=page.locator('[data-transcript-id="live-2"]');
    await second.getByRole('button',{name:'Delete recording 2',exact:true}).click();
    const dialog=page.getByRole('dialog');await dialog.getByRole('button',{name:'Keep it',exact:true}).click();
    assert.equal(await second.count(),1);
    await second.getByRole('button',{name:'Delete recording 2',exact:true}).click();
    await dialog.getByRole('button',{name:'Delete from app',exact:true}).click();await second.waitFor({state:'detached'});
    assert.equal(await first.count(),1);

    await page.evaluate(()=>window.pending.push({id:'pending-retry',name:'retry.wav'}));
    await page.getByRole('button',{name:'Refresh transcripts',exact:true}).click();
    const retry=page.locator('[data-recording-id="pending-retry"]');await retry.waitFor();
    await retry.locator('.pending-card-heading').click();
    const retryHistory=page.getByRole('region',{name:'Transcription history',exact:true}).getByRole('button').filter({hasText:'retry.wav'});
    await retryHistory.click();await retryHistory.click();
    assert.equal(await retry.locator('.pending-card-heading').getAttribute('aria-expanded'),'true','History always opens a draft rather than collapsing it');
    await retry.getByRole('button',{name:'Transcribe recording',exact:true}).click();
    await retry.waitFor({state:'detached'});await page.locator('[data-transcript-id^="batch-"]').waitFor();
    // Starting another message and navigating away must release the microphone.
    await page.getByRole('button',{name:'Start recording',exact:true}).click();
    await page.getByRole('button',{name:'Stop recording',exact:true}).waitFor();
    await page.locator('button.nav-item[aria-label="Home"]').click();
    assert.equal(await page.evaluate(()=>window.microphoneTracks.every(t=>t.readyState==='ended')),true);
  }finally{await browser.close();await new Promise((resolve,reject)=>server.httpServer.close(e=>e?reject(e):resolve()));}
});
