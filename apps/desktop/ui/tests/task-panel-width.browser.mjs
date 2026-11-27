import test from 'node:test';
import assert from 'node:assert/strict';
import {existsSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
import {preview} from 'vite';
import {chromium} from 'playwright-core';
import {seedVoiceHistory} from './voice-history-fixture.mjs';
const executable=process.env.CHROME_PATH||'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
test('task panels stay centered on wide displays and fit after history resizing, collapse and smaller windows',{skip:!existsSync(executable),timeout:60000},async()=>{
 const server=await preview({root:fileURLToPath(new URL('..',import.meta.url)),preview:{host:'127.0.0.1',port:0,open:false}}),browser=await chromium.launch({executablePath:executable,headless:true});
 try{
  const page=await browser.newPage({viewport:{width:2560,height:1200}}),errors=[];page.on('pageerror',error=>errors.push(error.message));await page.addInitScript(seedVoiceHistory,0);await page.goto(server.resolvedUrls.local[0]);
  async function check(label){
   await page.waitForTimeout(50);
   const bounds=await page.locator('.task-panel').evaluate(node=>{
    const box=node.getBoundingClientRect(),parent=node.parentElement.getBoundingClientRect();
    const separator=node.classList.contains('recording-main')?node.parentElement.querySelector('.history-resizer'):null;
    const left=separator?separator.getBoundingClientRect().right:parent.left;
    return {width:box.width,left:box.left,right:box.right,availableLeft:left,availableRight:parent.right,overflow:node.scrollWidth>node.clientWidth+1};
   });
   assert.ok(bounds.width<=1101,`${label}: panel stops expanding`);assert.ok(bounds.left>=bounds.availableLeft-1&&bounds.right<=bounds.availableRight+1,`${label}: panel fits available area`);
   assert.ok(Math.abs((bounds.left-bounds.availableLeft)-(bounds.availableRight-bounds.right))<=2,`${label}: equal space on both sides`);assert.equal(bounds.overflow,false,`${label}: no horizontal overflow`);
  }
  for(const label of ['Home','Text to Speech','Voice Cleanup','Voice Editing','Transcription','Dictation']){
   await page.setViewportSize({width:2560,height:1200});await page.locator(`button.nav-item[aria-label="${label}"]`).click();await page.locator('.task-panel').waitFor();await check(`${label} wide`);
   const separator=page.getByRole('separator');if(await separator.count()){
    await separator.focus();await page.keyboard.press('ArrowRight');await check(`${label} resized history`);
    await page.getByRole('button',{name:/^Hide .*history$/i}).click();await check(`${label} hidden history`);
   }
   if(label==='Text to Speech')await page.screenshot({path:'/tmp/ethiopic-centered-tts-wide.png'});
   for(const width of [1920,1100,820]){await page.setViewportSize({width,height:1000});await check(`${label} ${width}px`);}
  }
  assert.deepEqual(errors,[]);
 }finally{await browser.close();await new Promise((resolve,reject)=>server.httpServer.close(error=>error?reject(error):resolve()));}
});
