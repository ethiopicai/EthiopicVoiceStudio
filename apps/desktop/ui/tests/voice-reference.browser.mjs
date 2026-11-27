import test from 'node:test';
import assert from 'node:assert/strict';
import {existsSync,mkdtempSync,rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {fileURLToPath} from 'node:url';
import {createServer} from 'vite';
import {chromium} from 'playwright-core';
const executable=process.env.CHROME_PATH||'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
test('Speech references use the shared waveform, reference trimming is reversible and saved crops preserve original PCM and current metadata',{skip:!existsSync(executable),timeout:60000},async()=>{
 const cacheDir=mkdtempSync(join(tmpdir(),'ethiopic-reference-vite-')),server=await createServer({root:fileURLToPath(new URL('..',import.meta.url)),cacheDir,optimizeDeps:{noDiscovery:true,include:[]},server:{host:'127.0.0.1',port:0,strictPort:false,open:false}});await server.listen();
 const browser=await chromium.launch({executablePath:executable,headless:true,args:['--mute-audio']});
 try{
  let sharedStyle;
  for(const mode of ['speech']){
   const context=await browser.newContext({viewport:{width:1400,height:1050},colorScheme:'dark'}),page=await context.newPage(),errors=[];page.on('pageerror',error=>errors.push(error.message));
   let releaseProfiles;const profilesReady=new Promise(resolve=>releaseProfiles=resolve);
   await page.route('**/reference-voices/preview-v3/catalog.json',async route=>{await profilesReady;await route.continue();});
   await page.goto(`${server.resolvedUrls.local[0]}tests/fixtures/voice-reference.html?mode=${mode}`);
   const panel=page.getByRole('region',{name:'Reference recording',exact:true}),player=panel.locator('.waveform-player');await player.locator('.waveform-track svg').waitFor();
   assert.equal(await panel.locator('audio[controls]').count(),0,'native audio controls are a failure fallback only');
   const appearance=await player.evaluate(element=>{const s=getComputedStyle(element);return {background:s.backgroundColor,padding:s.padding,border:s.borderColor,radius:s.borderRadius};});if(sharedStyle)assert.deepEqual(appearance,sharedStyle);else sharedStyle=appearance;
   await page.getByLabel('Save a project voice profile',{exact:true}).fill('Original voice');
   const saveProfile=page.getByRole('button',{name:'Save reference profile',exact:true}),loadingBlocked=await saveProfile.isDisabled();releaseProfiles();
   assert.equal(loadingBlocked,true,'saving waits for the voice library so its refresh cannot replace a reference during trimming');
   await saveProfile.click();
   await page.getByRole('button',{name:'Open card: Original voice',exact:true}).waitFor();
   await panel.getByRole('button',{name:'Trim recording',exact:true}).click();
   let start=panel.getByRole('slider',{name:'Trim start',exact:true}),end=panel.getByRole('slider',{name:'Trim end',exact:true});
   await start.focus();await page.keyboard.press('ArrowRight');assert.ok(Number(await start.getAttribute('aria-valuenow'))>.09);
   const track=await player.locator('.waveform-track').boundingBox(),handle=await end.boundingBox();await page.mouse.move(handle.x+handle.width/2,handle.y+handle.height/2);await page.mouse.down();await page.mouse.move(track.x+track.width*.75,track.y+track.height/2,{steps:5});await page.mouse.up();assert.ok(Math.abs(Number(await end.getAttribute('aria-valuenow'))-3)<.05,'pointer handle changes the selected end');
   await panel.getByRole('button',{name:'Reset selection',exact:true}).click();assert.equal(await start.getAttribute('aria-valuenow'),'0');
   await panel.getByRole('spinbutton',{name:'Start (seconds)',exact:true}).fill('3');await panel.getByRole('spinbutton',{name:'End (seconds)',exact:true}).fill('1');assert.equal(await panel.getByRole('button',{name:'Use trimmed copy',exact:true}).isDisabled(),true);
   await panel.getByRole('spinbutton',{name:'Start (seconds)',exact:true}).fill('1');await panel.getByRole('spinbutton',{name:'End (seconds)',exact:true}).fill('3');
   await panel.getByRole('button',{name:'Cancel trim',exact:true}).click();assert.equal(await panel.getByRole('slider',{name:'Trim start',exact:true}).count(),0);
   await panel.getByRole('button',{name:'Trim recording',exact:true}).click();await panel.getByRole('spinbutton',{name:'Start (seconds)',exact:true}).fill('1');await panel.getByRole('spinbutton',{name:'End (seconds)',exact:true}).fill('3');
   await page.screenshot({path:`/tmp/ethiopic-${mode}-trim-controls.png`});
   await panel.getByRole('button',{name:'Play selection',exact:true}).click();await page.waitForFunction(()=>window.referenceFixture.span!==null);assert.deepEqual(await page.evaluate(()=>window.referenceFixture.span),[0,1,2]);
   await panel.getByRole('button',{name:'Use trimmed copy',exact:true}).click();await page.getByRole('button',{name:'Undo trim',exact:true}).waitFor();
   assert.equal(await page.getByRole('textbox',{name:'Exact words in the reference WAV',exact:true}).inputValue(),'እንዴት ናችሁ');
   await page.getByRole('button',{name:'Undo trim',exact:true}).click();await page.getByRole('checkbox',{name:'Select current voice profile',exact:true}).waitFor();
   assert.equal(await page.getByRole('textbox',{name:'Exact words in the reference WAV',exact:true}).inputValue(),'ሰላም እንዴት ናችሁ ዛሬ');
   await panel.getByRole('button',{name:'Trim recording',exact:true}).click();await panel.getByRole('spinbutton',{name:'Start (seconds)',exact:true}).fill('1');await panel.getByRole('spinbutton',{name:'End (seconds)',exact:true}).fill('3');await panel.getByRole('button',{name:'Use trimmed copy',exact:true}).click();
   await page.getByRole('textbox',{name:'Save a project voice profile',exact:true}).fill('Trimmed voice');await page.getByRole('button',{name:'Save reference profile',exact:true}).click();await page.getByRole('button',{name:'Open card: Trimmed voice',exact:true}).waitFor();
   const saved=await page.evaluate(async()=>{const profiles=await window.referenceFixture.listProfiles(),original=profiles.find(p=>p.name==='Original voice'),trimmed=profiles.find(p=>p.name==='Trimmed voice');return {original:Array.from(new Uint8Array(await original.reference.arrayBuffer())),trimmed:Array.from(new Uint8Array(await trimmed.reference.arrayBuffer())),context:trimmed.sourceMedia,text:trimmed.referenceText,sourceUnchanged:Array.from(new Uint8Array(await window.referenceFixture.file.arrayBuffer())).every((b,i)=>b===window.referenceFixture.bytes[i])};});
   assert.equal(saved.sourceUnchanged,true);assert.deepEqual(saved.original,await page.evaluate(()=>window.referenceFixture.bytes));assert.deepEqual(saved.trimmed.slice(44),saved.original.slice(44+32000,44+96000));assert.equal(saved.text,'እንዴት ናችሁ');assert.equal(saved.context.timingsCurrent,true);assert.equal(saved.context.words[0].start,1.2-1);assert.equal(saved.context.source.kind,'file');assert.deepEqual(saved.context.editedRegions[0].span_out,[1.3-1,1.5]);
   await page.screenshot({path:`/tmp/ethiopic-${mode}-reference-waveform.png`});assert.deepEqual(errors,[]);await context.close();
  }
  const context=await browser.newContext(),page=await context.newPage();await page.goto(`${server.resolvedUrls.local[0]}tests/fixtures/voice-reference.html?float=1`);
  const panel=page.getByRole('region',{name:'Reference recording',exact:true});await panel.getByRole('button',{name:'Trim recording',exact:true}).click();await panel.getByRole('spinbutton',{name:'Start (seconds)',exact:true}).fill('.5');await panel.getByRole('spinbutton',{name:'End (seconds)',exact:true}).fill('2.5');await panel.getByRole('button',{name:'Use trimmed copy',exact:true}).click();await page.getByRole('button',{name:'Undo trim',exact:true}).waitFor();
  assert.equal(await page.getByRole('textbox',{name:'Exact words in the reference WAV',exact:true}).inputValue(),'','a crop through words requires a matching transcript');
  await page.getByRole('textbox',{name:'Save a project voice profile',exact:true}).fill('Converted voice');await page.getByRole('button',{name:'Save reference profile',exact:true}).click();await page.getByRole('button',{name:'Open card: Converted voice',exact:true}).waitFor();
  assert.deepEqual(await page.evaluate(async()=>{const [p]=await window.referenceFixture.listProfiles(),bytes=await p.reference.arrayBuffer(),v=new DataView(bytes);return {format:v.getUint16(20,true),bits:v.getUint16(34,true),duration:v.getUint32(40,true)/2/v.getUint32(24,true)};}),{format:1,bits:16,duration:2});await context.close();
 }finally{await browser.close();await server.close();rmSync(cacheDir,{recursive:true,force:true});}
});
