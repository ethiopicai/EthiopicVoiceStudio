import test from 'node:test';
import assert from 'node:assert/strict';
import {existsSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
import {preview} from 'vite';
import {chromium} from 'playwright-core';
import {seedVoiceHistory} from './voice-history-fixture.mjs';
const executable=process.env.CHROME_PATH||'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
test('advanced controls reach generation, are locked during jobs, and saved settings can be reopened and reused',{skip:!existsSync(executable),timeout:60000},async()=>{
 const server=await preview({root:fileURLToPath(new URL('..',import.meta.url)),preview:{host:'127.0.0.1',port:0,open:false}}),browser=await chromium.launch({executablePath:executable,headless:true});
 try{
  const page=await browser.newPage({viewport:{width:1400,height:1100},colorScheme:'dark'}),errors=[];page.on('pageerror',error=>errors.push(error.message));
  await page.addInitScript(seedVoiceHistory,0);
  await page.addInitScript(()=>{if(window.self!==window.top)return;const base=window.__TAURI_INTERNALS__.invoke;window.controlRequests=[];window.engine='tts-flow_lm-90m-amh-fp32';window.controlBusy=false;window.__TAURI_INTERNALS__.invoke=async(command,args={})=>{
   if(command==='get_voice_generation_status')return {ready:true,consent:true,busy:window.controlBusy,provider:window.engine,requiresReferenceText:false,defaultVoice:true};
   if(command==='get_voice_generation_activity')return {busy:window.controlBusy,stage:'Generating your speech…',loaded:true};
   if(command==='get_default_voice')return {name:'Default voice.wav',bytes:window.voiceBytes};
   if(command==='synthesize_speech'){const input=args.input,project=await window.__TAURI_INTERNALS__.invoke('import_voice_edit_project',input);await window.__TAURI_INTERNALS__.invoke('voice_edit_action',{...input,projectId:project.id});return {groupId:project.id,projectId:project.id};}
   if(command==='import_voice_edit_project')return {id:`controls-${window.controlRequests.length+1}`,revision:0};
   if(command==='voice_edit_action'){window.controlRequests.push(structuredClone(args));window.controlBusy=true;return new Promise(resolve=>window.finishControlledGeneration=()=>{window.controlBusy=false;const settings={schemaVersion:1,seed:args.seed,...args.generationOptions};window.voiceGenerations.push({id:args.projectId,name:args.projectId+'.wav',text:args.text,revision:0,created:2,settings});resolve({settings,sample_rate:24000});});}
   return base(command,args);
  };});
  const nav=label=>page.locator(`button.nav-item[aria-label="${label}"]`);await page.goto(server.resolvedUrls.local[0]);await nav('Text to Speech').click();
  const advanced=page.locator('.speech-controls'),temperature=advanced.getByLabel('Temperature',{exact:true}),pause=advanced.getByLabel('Sentence pause (seconds)',{exact:true}),automatic=advanced.getByLabel('New variation each time',{exact:true});
  await advanced.waitFor();assert.equal(await advanced.evaluate(node=>node.open),false);await advanced.locator('summary').click();await temperature.waitFor();
  assert.equal(await temperature.inputValue(),'0.3');assert.equal(await pause.inputValue(),'0.2');assert.equal(await automatic.isChecked(),true);
  await temperature.fill('0.45');await pause.fill('0.75');await automatic.uncheck();await advanced.getByLabel('Variation seed',{exact:true}).fill('1234');
  await page.getByLabel('Script',{exact:true}).fill('ሰላም ጤና ይስጥልኝ።');await page.getByRole('button',{name:'Generate speech',exact:true}).click();await page.waitForFunction(()=>!!window.finishControlledGeneration);
  assert.equal(await temperature.isDisabled(),true);assert.equal(await automatic.isDisabled(),true);assert.equal(await advanced.getByRole('button',{name:'Reset defaults',exact:true}).isDisabled(),true);
  await page.evaluate(()=>window.finishControlledGeneration());await page.getByRole('button',{name:'Open generated speech: controls-1.wav',exact:true}).waitFor();
  const request=await page.evaluate(()=>window.controlRequests[0]);assert.deepEqual(request.generationOptions,{temperature:.45,sentencePause:.75});assert.equal(request.seed,1234);
  await nav('Home').click();await nav('Text to Speech').click();await page.getByRole('button',{name:'Open generated speech: controls-1.wav',exact:true}).click();
  const saved=page.locator('.take-settings');await saved.locator('summary').click();assert.match(await saved.innerText(),/0.45/);assert.match(await saved.innerText(),/0.75 seconds/);assert.match(await saved.innerText(),/1234/);
  await page.getByRole('button',{name:'Edit & regenerate',exact:true}).click();await advanced.locator('summary').click();
  assert.equal(await temperature.inputValue(),'0.45');assert.equal(await pause.inputValue(),'0.75');assert.equal(await automatic.isChecked(),false);assert.equal(await advanced.getByLabel('Variation seed',{exact:true}).inputValue(),'1234');
  await advanced.getByRole('button',{name:'Reset defaults',exact:true}).click();assert.equal(await temperature.inputValue(),'0.3');assert.equal(await pause.inputValue(),'0.2');assert.equal(await automatic.isChecked(),true);
  await temperature.fill('');assert.equal(await page.getByRole('button',{name:'Regenerate speech',exact:true}).isDisabled(),true);await advanced.getByRole('alert').waitFor();await temperature.fill('0.3');
  await page.screenshot({path:'/tmp/ethiopic-advanced-speech-controls.png'});
  await page.getByRole('button',{name:'New speech',exact:true}).click();await page.locator('.speech-controls summary').click();await page.getByLabel('New variation each time',{exact:true}).check();
  await page.getByRole('button',{name:'Generate speech',exact:true}).click();await page.waitForFunction(()=>window.controlRequests.length===2);await page.evaluate(()=>window.finishControlledGeneration());await page.getByRole('button',{name:'Open generated speech: controls-2.wav',exact:true}).waitFor();
  const seed=await page.evaluate(()=>window.controlRequests[1].seed);assert.ok(Number.isInteger(seed)&&seed>=0&&seed<=4294967295);
  await nav('Home').click();await page.evaluate(()=>window.engine='tts-codec_llm-0.6b-amh-fp16');await nav('Text to Speech').click();await advanced.locator('summary').click();
  await temperature.waitFor();assert.equal(await temperature.inputValue(),'0');assert.equal(await pause.inputValue(),'0.2');
  const speed=advanced.getByLabel('Speaking speed',{exact:true}),durationToggle=advanced.getByLabel('Set a target duration',{exact:true}),steps=advanced.getByLabel('Generation steps',{exact:true});
  assert.equal(await speed.inputValue(),'1');await speed.fill('0.8');await temperature.fill('0.25');await steps.selectOption('16');
  await durationToggle.check();assert.equal(await speed.isDisabled(),true);assert.equal(await pause.isDisabled(),true);await advanced.getByLabel('Target duration (seconds)',{exact:true}).fill('4');
  await automatic.uncheck();await advanced.getByLabel('Variation seed',{exact:true}).fill('77');
  await page.getByRole('button',{name:'Generate speech',exact:true}).click();await page.waitForFunction(()=>window.controlRequests.length===3);
  const gpuRequest=await page.evaluate(()=>window.controlRequests[2]);assert.deepEqual(gpuRequest.generationOptions,{temperature:.25,numSteps:16,duration:4});assert.equal(gpuRequest.seed,77);
  await page.evaluate(()=>window.finishControlledGeneration());await page.getByRole('button',{name:'Open generated speech: controls-3.wav',exact:true}).waitFor();
  await page.getByRole('button',{name:'Edit & regenerate',exact:true}).click();await advanced.locator('summary').click();
  assert.equal(await durationToggle.isChecked(),true);assert.equal(await advanced.getByLabel('Target duration (seconds)',{exact:true}).inputValue(),'4');assert.equal(await steps.inputValue(),'16');
  await durationToggle.uncheck();await speed.fill('1.2');await pause.fill('0.4');
  await page.screenshot({path:'/tmp/ethiopic-gpu-speech-controls.png'});
  assert.deepEqual(errors,[]);
 }finally{await browser.close();await new Promise((resolve,reject)=>server.httpServer.close(error=>error?reject(error):resolve()));}
});
