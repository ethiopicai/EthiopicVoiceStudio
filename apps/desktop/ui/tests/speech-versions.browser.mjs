import test from 'node:test';
import assert from 'node:assert/strict';
import {existsSync,readFileSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
import {preview} from 'vite';
import {chromium} from 'playwright-core';
import {seedVoiceHistory} from './voice-history-fixture.mjs';
const executable=process.env.CHROME_PATH||'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
test('speech cards regenerate with saved references, script, controls and version-specific actions',{skip:!existsSync(executable),timeout:60000},async()=>{
 const server=await preview({root:fileURLToPath(new URL('..',import.meta.url)),preview:{host:'127.0.0.1',port:0,open:false}}),browser=await chromium.launch({executablePath:executable,headless:true});
 try{
  const page=await browser.newPage({viewport:{width:1440,height:1100},colorScheme:'dark'}),errors=[];page.on('pageerror',error=>errors.push(error.message));
  await page.addInitScript(seedVoiceHistory,0);
  await page.addInitScript(()=>{if(window.self!==window.top)return;const base=window.__TAURI_INTERNALS__.invoke;window.speechRequests=[];window.speechVersions=[];window.speechPaths=[];window.speechReads=[];window.failSpeech=false;
   window.__TAURI_INTERNALS__.invoke=async(command,args={})=>{
    if(command==='get_voice_generation_status')return {ready:true,consent:true,busy:false,provider:'tts-flow_lm-90m-amh-fp32',requiresReferenceText:false,defaultVoice:true};
    if(command==='get_default_voice')return {name:'Default voice.wav',bytes:window.voiceBytes};
    if(command==='synthesize_speech'){
     const input=args.input;window.speechRequests.push(structuredClone(input));if(window.failSpeech)throw 'Generation failed. Earlier audio is saved.';
     const id=`speech-${window.speechVersions.length+1}`,groupId=input.groupId??id,settings={schemaVersion:1,...input.generationOptions,seed:input.seed};
     window.speechVersions.push({groupId,projectId:id,text:input.text,settings,created:Date.now()/1000,reference:{name:input.name,text:input.referenceText,voice:input.voice,sha256:'saved-reference-hash'},result:{model:'tts-flow_lm-90m-amh-fp32',model_version:'test',revision:'pinned-release'}});
     const summary={id:groupId,name:'My speech.wav',text:input.text,settings,revision:0,created:10,versionCount:window.speechVersions.length,currentVersion:id};window.voiceGenerations=window.voiceGenerations.filter(item=>item.id!==groupId).concat(summary);return {groupId,projectId:id};
    }
    if(command==='get_speech_versions'&&args.groupId==='speech-1')return structuredClone(window.speechVersions);
    if(command==='get_voice_edit_audio'){window.speechReads.push(args.projectId);return {name:'Audio.wav',bytes:window.voiceBytes};}
    if(command==='resolve_recording_transfer'){const version=window.speechVersions.find(item=>item.projectId===args.input.id);return {name:version.reference.name,bytes:window.voiceBytes,context:{schemaVersion:1,source:{kind:'editing',id:version.projectId},audioSha256:'hash',text:version.reference.text,words:[],timingsCurrent:false,editedRegions:[],lineage:[]}};}
    if(command==='copy_recording_transfer_path'||command==='get_recording_transfer_path'){window.speechPaths.push(args.input);return `/tmp/${args.input.id}.wav`;}
    return base(command,args);
   };
  });
  const nav=label=>page.locator(`button.nav-item[aria-label="${label}"]`);await page.goto(server.resolvedUrls.local[0]);await nav('Text to Speech').click();
  const script=page.getByLabel('Script',{exact:true});await script.fill('ሰላም ጤና ይስጥልኝ።');await page.getByRole('button',{name:'Generate speech',exact:true}).click();
  const card=page.locator('#voice-card-voice-project-speech-1');await card.getByRole('button',{name:'Edit & regenerate',exact:true}).waitFor();
  await script.fill('Unfinished new script');await card.getByRole('button',{name:'Edit & regenerate',exact:true}).click();assert.equal(await script.inputValue(),'ሰላም ጤና ይስጥልኝ።');
  await script.fill('ዛሬ እንዴት ናችሁ።');await page.getByLabel('Choose a voice',{exact:true}).selectOption('profile:profile-two');
  const controls=page.locator('.speech-controls');await controls.locator('summary').click();await controls.getByLabel('Temperature',{exact:true}).fill('0.6');await controls.getByLabel('Sentence pause (seconds)',{exact:true}).fill('0.8');await controls.getByLabel('New variation each time',{exact:true}).uncheck();await controls.getByLabel('Variation seed',{exact:true}).fill('99');
  await page.getByRole('button',{name:'Regenerate speech',exact:true}).click();await page.waitForFunction(()=>document.querySelector('[aria-label="Audio version"]')?.options.length===2);
  assert.equal(await card.getByLabel('Script',{exact:true}).inputValue(),'ዛሬ እንዴት ናችሁ።');assert.equal(await page.locator('#speech-composer').count(),1,'the only editor is inside the card');assert.equal(await card.locator('#speech-composer').count(),1);
  const request=await page.evaluate(()=>window.speechRequests[1]);assert.equal(request.groupId,'speech-1');assert.equal(request.baseVersion,'speech-1');assert.equal(request.expectedVersion,'speech-1');assert.equal(request.voice.label,'Voice two');assert.equal(request.name,'two.wav');assert.equal(request.seed,99);assert.deepEqual(request.generationOptions,{temperature:.6,sentencePause:.8});assert.equal(await page.locator('[data-generation-id="speech-2"]').count(),0);
  await script.fill('ሌላ ሙከራ።');await controls.getByLabel('Temperature',{exact:true}).fill('0.7');await card.getByRole('button',{name:'Regenerate speech',exact:true}).click();await page.waitForFunction(()=>document.querySelector('[aria-label="Audio version"]')?.options.length===3);
  const third=await page.evaluate(()=>window.speechRequests[2]);assert.equal(third.groupId,'speech-1');assert.equal(third.expectedVersion,'speech-2');assert.equal(third.baseVersion,'speech-2');assert.equal(third.generationOptions.temperature,.7);assert.equal(third.text,'ሌላ ሙከራ።');assert.equal(await page.locator('.generated-library [data-workspace-card]').count(),2,'iterations stay in one card alongside the existing fixture');
  const saved=card.locator('.take-settings');await saved.locator('summary').click();assert.match(await saved.innerText(),/Voice two/);assert.match(await saved.innerText(),/0.7/);assert.match(await saved.innerText(),/pinned-release/);
  const download=page.waitForEvent('download');await saved.getByRole('button',{name:'Export version metadata',exact:true}).click();const exported=await download;const metadata=JSON.parse(readFileSync(await exported.path(),'utf8'));assert.equal(metadata.projectId,'speech-3');assert.equal(metadata.settings.seed,99);assert.equal(metadata.reference.voice.label,'Voice two');
  await card.getByRole('button',{name:'Copy WAV path',exact:true}).click();await page.waitForFunction(()=>window.speechPaths.length===1);assert.equal(await page.evaluate(()=>window.speechPaths[0].id),'speech-3');
  const selector=card.getByRole('combobox',{name:'Audio version',exact:true});await selector.selectOption('speech-1');await page.waitForFunction(()=>window.speechReads.at(-1)==='speech-1');assert.equal(await card.getByLabel('Script',{exact:true}).inputValue(),'ሰላም ጤና ይስጥልኝ።');
  assert.equal(await script.inputValue(),'ሰላም ጤና ይስጥልኝ።');assert.equal(await page.getByLabel('Choose a voice',{exact:true}).inputValue(),'reference');
  await script.fill('An abandoned change');await page.getByRole('button',{name:'Done editing',exact:true}).click();assert.equal(await script.inputValue(),'Unfinished new script');
  await card.getByRole('button',{name:'Edit & regenerate',exact:true}).click();await page.evaluate(()=>window.failSpeech=true);await page.getByRole('button',{name:'Regenerate speech',exact:true}).click();await page.getByRole('alert').filter({hasText:'Generation failed.'}).waitFor();assert.equal(await selector.locator('option').count(),3);await page.getByRole('button',{name:'Done editing',exact:true}).click();
  await nav('Home').click();await nav('Text to Speech').click();await page.getByRole('button',{name:'Open generated speech: My speech.wav',exact:true}).click();await selector.waitFor();assert.equal(await selector.inputValue(),'speech-3');assert.equal(await selector.locator('option').count(),3);
  await card.getByRole('button',{name:'Edit & regenerate',exact:true}).click();await card.locator('#speech-composer').waitFor();
  for(const width of [1100,820]){await page.setViewportSize({width,height:1000});assert.equal(await card.evaluate(node=>node.scrollWidth<=node.clientWidth+1),true,`card editor fits ${width}px`);}
  await card.scrollIntoViewIfNeeded();await page.screenshot({path:'/tmp/ethiopic-inline-speech-versions.png'});await page.getByRole('button',{name:'New speech',exact:true}).click();assert.equal(await script.inputValue(),'Unfinished new script');
  const history=page.getByRole('region',{name:'Voice history',exact:true});await history.getByRole('checkbox',{name:'Select generated speech: My speech.wav',exact:true}).check();await history.getByRole('button',{name:'Delete selected',exact:true}).click();await page.getByRole('dialog').filter({visible:true}).getByRole('button',{name:'Delete from app',exact:true}).click();await card.waitFor({state:'detached'});assert.equal(await page.evaluate(()=>window.deleteRequests.at(-1)[0].id),'speech-1');assert.deepEqual(errors,[]);
 }finally{await browser.close();await new Promise((resolve,reject)=>server.httpServer.close(error=>error?reject(error):resolve()));}
});
