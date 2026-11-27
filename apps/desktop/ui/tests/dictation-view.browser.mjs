import test from 'node:test';
import assert from 'node:assert/strict';
import {existsSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
import {preview} from 'vite';
import {chromium} from 'playwright-core';
import {seedDictation} from './dictation-fixture.mjs';
const executable=process.env.CHROME_PATH||'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
test('Dictation opens at the latest card, preserves manual history browsing, reveals new recording and supports reduced motion',{skip:!existsSync(executable),timeout:60000},async()=>{
 const server=await preview({root:fileURLToPath(new URL('..',import.meta.url)),preview:{host:'127.0.0.1',port:0,open:false}}),browser=await chromium.launch({executablePath:executable,headless:true,args:['--use-fake-device-for-media-stream','--use-fake-ui-for-media-stream']});
 try{
  const page=await browser.newPage({viewport:{width:1100,height:780},reducedMotion:'no-preference'}),errors=[];page.on('pageerror',error=>errors.push(error.message));await page.addInitScript(seedDictation,20);await page.goto(server.resolvedUrls.local[0]);
  const nav=label=>page.locator(`button.nav-item[aria-label="${label}"]`);
  async function visibleCard(id){await page.waitForFunction(id=>{const card=document.getElementById(id),scroller=card?.closest('[data-card-scroller]');if(!card||!scroller)return false;const a=card.getBoundingClientRect(),b=scroller.getBoundingClientRect();return a.top<b.bottom&&a.bottom>b.top;},id);}
  await nav('Dictation').click();const latest=page.locator('#transcript-old-20');await latest.getByRole('checkbox',{name:'Highlight words during playback',exact:true}).waitFor();await visibleCard('transcript-old-20');assert.equal(await latest.getByRole('checkbox',{name:'Highlight words during playback',exact:true}).isChecked(),true);assert.ok(await page.locator('.recording-conversation').evaluate(node=>node.scrollTop)>0);
  const history=page.getByRole('region',{name:'Transcription history',exact:true});await history.getByRole('button').filter({hasText:'Dictation-old-1.wav'}).click();await visibleCard('transcript-old-1');await page.getByRole('button',{name:'Refresh transcripts',exact:true}).click();await visibleCard('transcript-old-1');assert.equal(await page.locator('#transcript-old-1 .transcript-card-heading').getAttribute('aria-expanded'),'true');
  await page.getByRole('button',{name:'Start recording',exact:true}).click();const stop=page.getByRole('button',{name:'Stop recording',exact:true});await stop.waitFor();await visibleCard('dictation-live');await page.locator('#dictation-live.is-recording').waitFor();
  assert.match(await stop.evaluate(node=>getComputedStyle(node,'::before').animationName),/recording-ripple/);assert.match(await page.locator('#dictation-live').evaluate(node=>getComputedStyle(node).animationName),/recording-border/);
  await page.screenshot({path:'/tmp/ethiopic-dictation-recording-pulse.png'});
  await page.emulateMedia({reducedMotion:'reduce'});assert.equal(await stop.evaluate(node=>getComputedStyle(node,'::before').display),'none');assert.equal(await stop.evaluate(node=>getComputedStyle(node).animationName),'none');assert.equal(await page.locator('#dictation-live').evaluate(node=>getComputedStyle(node).animationName),'none');
  await stop.click();const saved=page.locator('#transcript-live-1');await saved.getByRole('checkbox',{name:'Highlight words during playback',exact:true}).waitFor();await visibleCard('transcript-live-1');assert.equal(await page.locator('#dictation-live').count(),0);assert.equal(await page.locator('[data-transcript-id]').count(),21);
  await nav('Home').click();await nav('Dictation').click();await saved.getByRole('checkbox',{name:'Highlight words during playback',exact:true}).waitFor();await visibleCard('transcript-live-1');assert.equal(await saved.getByRole('checkbox',{name:'Highlight words during playback',exact:true}).isChecked(),true);
  await page.screenshot({path:'/tmp/ethiopic-dictation-latest-card.png'});assert.deepEqual(errors,[]);
 }finally{await browser.close();await new Promise((resolve,reject)=>server.httpServer.close(error=>error?reject(error):resolve()));}
});
