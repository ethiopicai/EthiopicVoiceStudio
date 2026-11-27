import test from 'node:test';
import assert from 'node:assert/strict';
import {existsSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
import {preview} from 'vite';
import {chromium} from 'playwright-core';
const executable=process.env.CHROME_PATH||'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';

test('General settings persist theme and microphone, track device changes and explain dictation startup failures',{skip:!existsSync(executable)},async()=>{
  const server=await preview({root:fileURLToPath(new URL('..',import.meta.url)),preview:{host:'127.0.0.1',port:0,open:false}});
  const browser=await chromium.launch({executablePath:executable,headless:true,args:['--use-fake-device-for-media-stream','--use-fake-ui-for-media-stream']});
  try{
    const page=await browser.newPage({viewport:{width:1200,height:900},colorScheme:'dark'});
    const errors=[];page.on('pageerror',error=>errors.push(error.message));
    await page.addInitScript(()=>{
      window.microphoneCalls=[];window.microphoneTracks=[];window.deviceLabels=false;window.usbAvailable=true;window.failure='';window.pollCalls=0;
      navigator.mediaDevices.enumerateDevices=async()=>[
        {kind:'audioinput',deviceId:'default',label:window.deviceLabels?'Default — Built-in microphone':''},
        {kind:'audioinput',deviceId:'built-in',label:window.deviceLabels?'Built-in microphone':''},
        ...(window.usbAvailable?[{kind:'audioinput',deviceId:'usb-input',label:window.deviceLabels?'USB microphone':''}]:[])
      ];
      const get=navigator.mediaDevices.getUserMedia.bind(navigator.mediaDevices);
      navigator.mediaDevices.getUserMedia=async constraints=>{
        window.microphoneCalls.push(constraints);
        if(constraints.audio.deviceId?.exact==='usb-input'&&!window.usbAvailable)throw new DOMException('missing','OverconstrainedError');
        const {deviceId,...audio}=constraints.audio;
        const stream=await get({audio});window.deviceLabels=true;window.microphoneTracks.push(...stream.getTracks());return stream;
      };
      window.__TAURI_INTERNALS__={invoke:async(command,args)=>{
        if(command==='system_summary')return {os:'macos',arch:'aarch64',logicalCpus:8,totalMemoryBytes:16*1024**3};
        if(command==='get_asr_status')return {state:'ready',message:'Ready',modelVersion:'v0.2'};
        if(command==='get_fixture_progress')return {state:'idle',downloadedBytes:0,totalBytes:0};
        if(command==='list_transcripts'||command==='list_transcription_recordings')return [];
        if(command==='get_active_dictation_session')return null;
        if(command==='start_dictation'){
          if(window.failure==='reject')throw 'Another voice task is running. Try again when it finishes.';
          return {id:'test',state:window.failure==='loading'?'loading':'ready',stage:'Getting voice tools ready',modelVersion:'v0.2',finalWords:[],partialWords:[]};
        }
        if(command==='get_dictation_session'){
          window.pollCalls++;
          if(window.failure==='loading'){await new Promise(resolve=>setTimeout(resolve,800));return {id:'test',state:'failed',error:'Startup needs more available memory.',stage:'Failed',finalWords:[],partialWords:[]};}
          return {id:'test',state:'recording',stage:'Listening',finalWords:[],partialWords:[]};
        }
        if(command==='cancel_dictation'||command==='feed_dictation')return;
        throw Error('Outside this test');
      }};
    });
    const settings=async()=>page.getByRole('button',{name:'Settings',exact:true}).click();
    const theme=page.getByRole('combobox',{name:'Theme',exact:true});
    const mic=page.getByRole('combobox',{name:'Microphone',exact:true});
    await page.goto(server.resolvedUrls.local[0]);await settings();
    await mic.locator('option[value="built-in"]').waitFor({state:'attached'});
    assert.equal(await page.evaluate(()=>window.microphoneCalls.length),0,'opening settings never records');
    await page.getByRole('button',{name:'Allow microphone access',exact:true}).click();
    await mic.locator('option').filter({hasText:'USB microphone'}).waitFor({state:'attached'});
    assert.equal(await page.evaluate(()=>window.microphoneTracks.every(t=>t.readyState==='ended')),true);
    await mic.selectOption('usb-input');
    await theme.selectOption('light');
    assert.equal(await page.locator('html').getAttribute('data-theme'),'light');
    await page.screenshot({path:'/tmp/ethiopic-settings-light.png'});
    await page.reload();await settings();
    assert.equal(await mic.inputValue(),'usb-input');assert.equal(await theme.inputValue(),'light');
    await theme.selectOption('system');
    assert.equal(await page.locator('html').getAttribute('data-theme'),'dark');
    await page.emulateMedia({colorScheme:'light'});
    await page.waitForFunction(()=>document.documentElement.dataset.theme==='light');
    await theme.selectOption('dark');
    assert.equal(await page.locator('html').getAttribute('data-theme'),'dark');
    await page.screenshot({path:'/tmp/ethiopic-settings-dark.png'});
    await page.locator('button.nav-item[aria-label="Dictation"]').click();
    await page.getByRole('button',{name:'Start recording',exact:true}).click();
    await page.getByRole('button',{name:'Stop recording',exact:true}).waitFor();
    assert.deepEqual(await page.evaluate(()=>window.microphoneCalls.at(-1).audio.deviceId),{exact:'usb-input'});
    await settings();
    assert.equal(await page.evaluate(()=>window.microphoneTracks.every(t=>t.readyState==='ended')),true,'navigation releases recording input');
    await page.evaluate(()=>{window.usbAvailable=false;navigator.mediaDevices.dispatchEvent(new Event('devicechange'));});
    await mic.locator('option').filter({hasText:'unavailable'}).waitFor({state:'attached'});
    assert.equal(await mic.inputValue(),'usb-input','device removal must not silently record from another input');
    await page.locator('button.nav-item[aria-label="Dictation"]').click();
    await page.getByRole('button',{name:'Start recording',exact:true}).click();
    await page.getByRole('alert').filter({hasText:'The selected microphone is unavailable.'}).waitFor();
    await settings();await mic.selectOption('');
    for(const [failure,message] of [['reject','Another voice task is running.'],['loading','Startup needs more available memory.']]){
      await page.evaluate(value=>{window.failure=value;window.pollCalls=0;},failure);
      await page.locator('button.nav-item[aria-label="Dictation"]').click();
      const count=await page.evaluate(()=>window.microphoneCalls.length);
      await page.getByRole('button',{name:'Start recording',exact:true}).click();
      await page.getByRole('alert').filter({hasText:message}).waitFor();
      assert.equal(await page.evaluate(()=>window.microphoneCalls.length),count,'do not open input before live model is ready');
      if(failure==='loading')assert.equal(await page.evaluate(()=>window.pollCalls),1,'loading loop owns polling, so failures cannot race');
      await settings();
    }
    await page.evaluate(()=>window.failure='');
    await page.locator('button.nav-item[aria-label="Dictation"]').click();
    await page.getByRole('button',{name:'Start recording',exact:true}).click();
    await page.getByRole('button',{name:'Stop recording',exact:true}).waitFor();
    assert.equal(await page.evaluate(()=>window.microphoneCalls.at(-1).audio.deviceId),undefined);
    await settings();assert.equal(await page.evaluate(()=>window.microphoneTracks.every(t=>t.readyState==='ended')),true);
    assert.deepEqual(errors,[]);
  }finally{await browser.close();await new Promise((resolve,reject)=>server.httpServer.close(e=>e?reject(e):resolve()));}
});
