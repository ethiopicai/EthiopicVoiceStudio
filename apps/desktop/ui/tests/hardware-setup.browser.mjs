import {seedDownloadReview} from './download-review-fixture.mjs';
import test from 'node:test';
import assert from 'node:assert/strict';
import {existsSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
import {preview} from 'vite';
import {chromium} from 'playwright-core';
import {seedVoiceHistory} from './voice-history-fixture.mjs';
const executable=process.env.CHROME_PATH||'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
test('hardware overview stays read-only and updates automatically when voice tools change',{skip:!existsSync(executable),timeout:60000},async()=>{
 const server=await preview({root:fileURLToPath(new URL('..',import.meta.url)),preview:{host:'127.0.0.1',port:0,open:false}}),browser=await chromium.launch({executablePath:executable,headless:true});
 try{
 const page=await browser.newPage({viewport:{width:1400,height:1000}}),errors=[];page.on('pageerror',e=>errors.push(e.message));await page.addInitScript(seedVoiceHistory,0);
 await page.addInitScript(()=>{if(window.self!==window.top)return;const base=window.__TAURI_INTERNALS__.invoke;window.setupCalls=[];window.failHardware=false;
 const feature=(id,title,model)=>({id,title,model,downloads:[model],modelBytes:400*1024**2,requiredStorageBytes:800*1024**2,ready:false,canInstall:id==='speech',requiresConsent:id==='speech',activeVersion:null,message:id==='speech'?'Ready to download the selected speech tools.':'Compatible runtime is not released yet.'});
 window.hardwarePlan={supported:true,reason:'Tools are matched to this computer.',hardware:{os:'macos',arch:'aarch64',totalRamBytes:4*1024**3,freeRamBytes:300*1024**2,physicalCores:4,avx2:null,nvidia:[]},osVersion:'15.0',freeStorageBytes:10*1024**3,routing:{tier:'apple-arm',downloadBytes:1600*1024**2,downloads:['asr-wav2vec_ctc-1b-amh-int8_onnx','asr-wav2vec_ctc-300m-amh-int8_onnx','tts-flow_lm-90m-amh-fp32'],warnings:['Other apps are using memory. Setup can continue; close unused apps before processing speech.']},features:[feature('transcription','Transcription','asr-wav2vec_ctc-1b-amh-int8_onnx'),feature('dictation','Dictation','asr-wav2vec_ctc-300m-amh-int8_onnx'),feature('speech','Text to Speech','tts-flow_lm-90m-amh-fp32')]};
 window.__TAURI_INTERNALS__.invoke=async(command,args={})=>{window.setupCalls.push(command);if(command==='get_setup_plan'){if(window.failHardware)throw 'Hardware unavailable';return structuredClone(window.hardwarePlan);}if(command==='get_voice_setup')return {state:'idle',stage:'Runtime not released yet.',downloadedBytes:0,totalBytes:0,canInstall:false};if(command==='accept_voice_edit_consent'){window.hardwarePlan.features[2].requiresConsent=false;return;}if(command==='prepare_voice_generation')return new Promise((resolve,reject)=>{window.resolveSetup=()=>{window.hardwarePlan.features[2].ready=true;window.hardwarePlan.features[2].message='Installed tools are ready.';resolve();};window.rejectSetup=reject;});if(command==='get_voice_generation_activity')return {busy:true,stage:'Downloading speech tools · 25%',loaded:false};if(command==='cancel_voice_edit'){window.rejectSetup('paused');return;}return base(command,args);};
 });
 await page.addInitScript(seedDownloadReview);
 await page.goto(server.resolvedUrls.local[0]);await page.getByRole('button',{name:'Settings',exact:true}).click();const setup=page.getByRole('region',{name:'Automatic model setup',exact:true});await setup.getByText('Apple Silicon Mac',{exact:true}).waitFor();
 assert.equal(await page.evaluate(()=>window.setupCalls.filter(c=>c==='get_setup_plan').length),1);
 assert.equal(await page.evaluate(()=>window.setupCalls.includes('prepare_voice_generation')),false,'inspection never downloads');
 assert.equal(await setup.locator('.model-choice').first().isVisible(),false,'model IDs stay under details');
 await setup.locator('summary').click();await setup.getByText(/Recommended: tts-flow_lm-90m-amh-fp32/).waitFor();
 assert.equal(await setup.getByRole('button',{name:'Download speech tools',exact:true}).count(),0,'one guided setup owns all downloads');
 await page.evaluate(()=>{window.hardwarePlan.features[2].ready=true;window.hardwarePlan.features[2].message='Installed tools are ready.';window.dispatchEvent(new Event('studio-tools-changed'));});
 await setup.getByText('Installed tools are ready.',{exact:true}).waitFor();
 assert.equal(await page.evaluate(()=>window.setupCalls.includes('prepare_voice_generation')),false);
 await page.screenshot({path:'/tmp/ethiopic-hardware-setup.png'});
 await page.evaluate(()=>{window.hardwarePlan.supported=false;window.hardwarePlan.hardware.os='windows';window.hardwarePlan.hardware.arch='x86_64';window.hardwarePlan.reason='Windows setup will be released separately.';for(const feature of window.hardwarePlan.features){feature.ready=false;feature.canInstall=false;}});await setup.getByRole('button',{name:'Check hardware again',exact:true}).click();await setup.getByText('Windows setup will be released separately.',{exact:true}).waitFor();assert.equal(await setup.getByRole('button',{name:'Download speech tools',exact:true}).count(),0);
 await page.evaluate(()=>window.failHardware=true);await setup.getByRole('button',{name:'Check hardware again',exact:true}).click();await setup.getByRole('alert').filter({hasText:'Hardware could not be checked.'}).waitFor();assert.deepEqual(errors,[]);
 }finally{await browser.close();await new Promise((resolve,reject)=>server.httpServer.close(error=>error?reject(error):resolve()));}
});
