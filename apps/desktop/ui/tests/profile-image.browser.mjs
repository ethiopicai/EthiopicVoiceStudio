import test from 'node:test';
import assert from 'node:assert/strict';
import {existsSync,readFileSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
import {preview} from 'vite';
import {chromium} from 'playwright-core';
import {seedVoiceHistory} from './voice-history-fixture.mjs';
const policy=JSON.parse(readFileSync(new URL('../../src-tauri/tauri.conf.json',import.meta.url),'utf8')).app.security.csp;
const executable=process.env.CHROME_PATH||'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
test('voice photos upload by clicking the circle, persist in the separate reference panel, and invalid replacements keep the saved image',{skip:!existsSync(executable),timeout:60000},async()=>{
 const server=await preview({root:fileURLToPath(new URL('..',import.meta.url)),preview:{host:'127.0.0.1',port:0,open:false}}),browser=await chromium.launch({executablePath:executable,headless:true});
 try{
  const page=await browser.newPage({viewport:{width:1440,height:1000},colorScheme:'dark'}),errors=[];page.on('pageerror',error=>errors.push(error.message));await page.addInitScript(seedVoiceHistory,0);
  await page.route(server.resolvedUrls.local[0],async route=>{const response=await route.fetch();await route.fulfill({response,body:(await response.text()).replace('<head>',`<head><meta http-equiv="Content-Security-Policy" content="${policy}">`)});});
  await page.goto(server.resolvedUrls.local[0]);
  const nav=label=>page.locator(`button.nav-item[aria-label="${label}"]`);await nav('Text to Speech').click();await page.locator('#voice-library > summary').click();
  const card=page.locator('#voice-card-voice-profile-profile-one'),avatar=card.getByRole('button',{name:'Change profile image for Voice one',exact:true});await avatar.waitFor();assert.match(await avatar.locator('img').getAttribute('src'),/default-male\.jpg$/);
  await card.locator('summary[aria-label="Choose default profile image for Voice one"]').click();await card.getByRole('button',{name:'Use female portrait for Voice one',exact:true}).click();await page.waitForFunction(()=>document.querySelector('#voice-card-voice-profile-profile-one .profile-avatar img')?.src.endsWith('/profile-images/default-female.jpg'));
  await nav('Home').click();await nav('Text to Speech').click();await page.locator('#voice-library > summary').click();await avatar.waitFor();assert.match(await avatar.locator('img').getAttribute('src'),/default-female\.jpg$/);
  const photo=async color=>Buffer.from(await page.evaluate(color=>{const canvas=document.createElement('canvas');canvas.width=600;canvas.height=400;const context=canvas.getContext('2d');context.fillStyle=color;context.fillRect(0,0,600,400);return canvas.toDataURL('image/png').split(',')[1];},color),'base64');
  const original=await photo('#317d6a'),originalCopy=Buffer.from(original);
  let chooser=page.waitForEvent('filechooser');await avatar.focus();await page.keyboard.press('Space');await (await chooser).setFiles({name:'portrait.png',mimeType:'image/png',buffer:original});
  await page.waitForFunction(()=>document.querySelector('#voice-card-voice-profile-profile-one .profile-avatar img')?.src.startsWith('data:image/jpeg;base64,'),{},{timeout:5000});const first=await avatar.locator('img').getAttribute('src');assert.ok(first.startsWith('data:image/jpeg;base64,'));assert.ok(first.length<200_000);assert.deepEqual(original,originalCopy);
  assert.equal(await avatar.locator('img').evaluate(img=>img.naturalWidth),256);assert.equal(await avatar.locator('img').evaluate(img=>getComputedStyle(img).borderRadius),'50%');
  const history=page.getByRole('region',{name:'Voice history',exact:true});assert.equal(await history.locator('[data-profile-id],.profile-avatar').count(),0);assert.equal(await history.locator('.history-entry').count(),1);
  chooser=page.waitForEvent('filechooser');await avatar.click();await (await chooser).setFiles({name:'bad.txt',mimeType:'text/plain',buffer:Buffer.from('invalid')});await card.getByRole('alert').filter({hasText:'Choose a JPG, PNG or WebP image.'}).waitFor();assert.equal(await avatar.locator('img').getAttribute('src'),first);
  chooser=page.waitForEvent('filechooser');await avatar.click();await (await chooser).setFiles({name:'broken.png',mimeType:'image/png',buffer:Buffer.from('broken PNG')});await card.getByRole('alert').filter({hasText:'could not be opened'}).waitFor();assert.equal(await avatar.locator('img').getAttribute('src'),first);
  await nav('Home').click();await nav('Text to Speech').click();await page.locator('#voice-library > summary').click();await avatar.locator('img').waitFor();assert.equal(await avatar.locator('img').getAttribute('src'),first);
  chooser=page.waitForEvent('filechooser');await avatar.click();await (await chooser).setFiles({name:'replacement.png',mimeType:'image/png',buffer:await photo('#ac7441')});await page.waitForFunction(first=>document.querySelector('#voice-card-voice-profile-profile-one .profile-avatar img')?.src!==first,first);
  const second=await avatar.locator('img').getAttribute('src');assert.notEqual(second,first);assert.equal(await history.locator('.history-entry').count(),1);
  const saved=await page.evaluate(async()=>{const db=await new Promise(resolve=>{const request=indexedDB.open('ethiopic-voice-profiles');request.onsuccess=()=>resolve(request.result);});const profile=await new Promise(resolve=>{const request=db.transaction('profiles','readonly').objectStore('profiles').get('profile-one');request.onsuccess=()=>resolve(request.result);});db.close();return {image:profile.image,name:profile.reference.name,bytes:(await profile.reference.arrayBuffer()).byteLength,text:profile.referenceText};});
  assert.equal(saved.image,second);assert.equal(saved.name,'one.wav');assert.equal(saved.bytes,32044);assert.equal(saved.text,'ጤና');
  await page.screenshot({path:'/tmp/ethiopic-profile-images.png'});assert.deepEqual(errors,[]);
 }finally{await browser.close();await new Promise((resolve,reject)=>server.httpServer.close(error=>error?reject(error):resolve()));}
});

test('bundled preview voices install and all persona portraits render under the packaged app CSP',{skip:!existsSync(executable),timeout:60000},async()=>{
 const server=await preview({root:fileURLToPath(new URL('..',import.meta.url)),preview:{host:'127.0.0.1',port:0,open:false,headers:{'Content-Security-Policy':policy}}}),browser=await chromium.launch({executablePath:executable,headless:true});
 try{
  const page=await browser.newPage({viewport:{width:1440,height:1000}}),violations=[];
  await page.addInitScript(seedVoiceHistory,0);
  await page.exposeFunction('recordPolicyViolation',value=>violations.push(value));
  await page.addInitScript(()=>document.addEventListener('securitypolicyviolation',event=>window.recordPolicyViolation(`${event.violatedDirective}: ${event.blockedURI}`)));
  await page.goto(server.resolvedUrls.local[0]);
  // Simulate a saved library that has not installed the preview pack yet.
  await page.evaluate(()=>new Promise((resolve,reject)=>{const request=indexedDB.open('ethiopic-voice-profiles');request.onerror=()=>reject(request.error);request.onsuccess=()=>{const db=request.result,tx=db.transaction('presetPacks','readwrite');tx.objectStore('presetPacks').clear();tx.oncomplete=()=>{db.close();resolve();};tx.onerror=()=>reject(tx.error);};}));
  await page.locator('button.nav-item[aria-label="Text to Speech"]').click();
  await page.locator('#voice-library > summary').click();
  const portraits=page.locator('#voice-library img[src^="/profile-images/presets/"]');
  await page.waitForFunction(()=>document.querySelectorAll('#voice-library img[src^="/profile-images/presets/"]').length===17);
  assert.equal(await portraits.count(),17);
  await page.waitForFunction(()=>[...document.querySelectorAll('#voice-library img[src^="/profile-images/presets/"]')].every(img=>img.complete&&img.naturalWidth===256&&img.naturalHeight===256));
  assert.equal(new Set(await portraits.evaluateAll(images=>images.map(image=>image.getAttribute('src')))).size,14);
  assert.equal(await page.getByRole('region',{name:'Voice history',exact:true}).locator('.profile-avatar').count(),0);
  assert.deepEqual(violations,[]);
 }finally{await browser.close();await new Promise((resolve,reject)=>server.httpServer.close(error=>error?reject(error):resolve()));}
});
