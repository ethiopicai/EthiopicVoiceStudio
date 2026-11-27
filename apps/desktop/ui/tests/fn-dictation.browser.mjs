import test from 'node:test';
import assert from 'node:assert/strict';
import {existsSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
import {preview} from 'vite';
import {chromium} from 'playwright-core';
import {observeCaptureSamples} from './audio-capture-fixture.mjs';
const executable=process.env.CHROME_PATH||'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';

test('hold Fn uses default input, retains speech through warmup, inserts only saved results and cancels safely',{skip:!existsSync(executable)},async()=>{
  const server=await preview({root:fileURLToPath(new URL('..',import.meta.url)),preview:{host:'127.0.0.1',port:0,open:false}});
  const browser=await chromium.launch({executablePath:executable,headless:true,args:['--use-fake-device-for-media-stream','--use-fake-ui-for-media-stream']});
  try{
    const page=await browser.newPage({viewport:{width:1100,height:850}});const errors=[];page.on('pageerror',e=>errors.push(e.message));
    await page.addInitScript(observeCaptureSamples);
    await page.addInitScript(()=>{
      if(window.self!==window.top)return;
      localStorage.setItem('ethiopic-studio.preferences.v1',JSON.stringify({theme:'dark',microphoneId:'chosen-input'}));
      window.callbacks={};window.handlers={};window.tracks=[];window.inputs=[];window.feeds=[];window.finishes=[];window.saved=[];window.starts=0;window.delayInput=0;window.rejectModel=false;window.enabled=false;
      const get=navigator.mediaDevices.getUserMedia.bind(navigator.mediaDevices);
      navigator.mediaDevices.getUserMedia=async options=>{window.inputs.push(options);if(window.delayInput)await new Promise(resolve=>setTimeout(resolve,window.delayInput));const stream=await get(options);window.tracks.push(...stream.getTracks());return stream;};
      window.sendFn=(requestId,pressed,cancelled=false)=>window.callbacks[window.handlers['dictation-shortcut']]({event:'dictation-shortcut',id:1,payload:{requestId,pressed,cancelled}});
      const status=()=>({supported:true,enabled:window.enabled,requested:window.enabled,inputMonitoring:true,accessibility:true,active:null});
      const wav=()=>{const b=new Uint8Array(32044),v=new DataView(b.buffer);for(const [at,text] of [[0,'RIFF'],[8,'WAVE'],[12,'fmt '],[36,'data']])for(let i=0;i<text.length;i++)v.setUint8(at+i,text.charCodeAt(i));v.setUint32(4,b.length-8,true);v.setUint32(16,16,true);v.setUint16(20,1,true);v.setUint16(22,1,true);v.setUint32(24,16000,true);v.setUint32(28,32000,true);v.setUint16(32,2,true);v.setUint16(34,16,true);v.setUint32(40,32000,true);return Array.from(b);};
      window.__TAURI_EVENT_PLUGIN_INTERNALS__={unregisterListener:()=>{}};
      window.__TAURI_INTERNALS__={transformCallback:callback=>{const id=Object.keys(window.callbacks).length+1;window.callbacks[id]=callback;return id;},invoke:async(command,args)=>{
        if(command==='plugin:event|listen'){window.handlers[args.event]=args.handler;return 1;}
        if(command==='plugin:event|unlisten')return;
        if(command==='get_fn_shortcut_status')return status();
        if(command==='set_fn_shortcut_enabled'){window.enabled=args.enabled;return status();}
        if(command==='system_summary')return {os:'macos',arch:'aarch64',logicalCpus:8,totalMemoryBytes:4*1024**3};
        if(command==='get_asr_status')return {state:'ready',message:'Ready',modelVersion:'v0.2'};
        if(command==='get_fixture_progress')return {state:'idle',downloadedBytes:0,totalBytes:0};
        if(command==='get_active_dictation_session')return null;
        if(command==='list_transcripts')return window.saved;
        if(command==='list_transcription_recordings')return [];
        if(command==='get_transcript')return window.saved.find(t=>t.id===args.id);
        if(command==='get_transcript_audio')return {name:'Fn.wav',bytes:wav()};
        if(command==='start_dictation'){
          window.starts++;if(window.rejectModel)throw 'The private dictation runtime could not start. Repair voice tools.';
          window.modelReady=false;window.session={id:`fn-${window.starts}`,state:'loading',stage:'Loading voice tools',modelVersion:'v0.2',finalWords:[],partialWords:[]};return {...window.session};
        }
        if(command==='bind_fn_dictation'){window.bound=args;return;}
        if(command==='update_fn_overlay')return;
        if(command==='get_dictation_session'){if(window.session.state==='loading'&&window.modelReady)window.session.state='ready';return {...window.session};}
        if(command==='feed_dictation'){if(!['ready','recording'].includes(window.session.state))throw Error('sent audio before ready');window.feeds.push(args.samples.length);return;}
        if(command==='stop_dictation'){
          const id=window.session.id;window.saved.push({id,name:'Fn.wav',text:'ሰላም',model_version:'v0.2',created_at_ms:Date.now(),revision:1,words:[{id:0,word:'ሰላም',corrected:'ሰላም',start:0,end:.2}],corrections:[]});
          window.session={...window.session,state:'completed',stage:'Saved',transcriptId:id};return;
        }
        if(command==='cancel_dictation'){window.session.state='cancelled';return;}
        if(command==='finish_fn_dictation'){window.finishes.push(args);if(args.transcriptId&&!window.saved.some(t=>t.id===args.transcriptId))throw Error('unsaved transcript');return args.transcriptId?'Inserted into your app and saved in Studio.':'';}
        throw Error('Outside this test');
      }};
    });
    await page.goto(server.resolvedUrls.local[0]);
    await page.waitForFunction(()=>!!window.handlers['dictation-shortcut']);
    await page.getByRole('button',{name:'Settings',exact:true}).click();
    await page.getByRole('button',{name:'Enable Fn dictation',exact:true}).click();
    await page.getByText('Ready · hold Fn',{exact:true}).waitFor();
    // A 4 GiB computer is allowed to try loading; the shortcut overrides a chosen mic.
    await page.locator('button.nav-item[aria-label="Home"]').click();
    await page.evaluate(()=>window.sendFn(1,true));
    await page.getByRole('button',{name:'Stop recording',exact:true}).waitFor();
    assert.equal(await page.evaluate(()=>window.inputs.at(-1).audio.deviceId),undefined);
    await page.waitForFunction(()=>window.capturedSamples>2048);
    assert.equal(await page.evaluate(()=>window.feeds.length),0);
    await page.evaluate(()=>{window.sendFn(1,false);window.modelReady=true;});
    await page.locator('[data-transcript-id="fn-1"]').waitFor();
    assert.equal(await page.evaluate(()=>window.feeds.reduce((a,b)=>a+b,0)>400),true);
    assert.deepEqual(await page.evaluate(()=>window.finishes),[{requestId:1,transcriptId:'fn-1'}]);
    assert.equal(await page.evaluate(()=>window.tracks.every(t=>t.readyState==='ended')),true);
    await page.getByText('Inserted into your app and saved in Studio.',{exact:true}).waitFor();
    // Release while the permission/device request is pending: no late microphone leak.
    await page.evaluate(()=>{window.delayInput=500;window.sendFn(2,true);});
    await page.waitForFunction(()=>window.inputs.length===2);
    await page.evaluate(()=>window.sendFn(2,false));await page.waitForTimeout(700);
    assert.equal(await page.evaluate(()=>window.tracks.every(t=>t.readyState==='ended')),true);
    assert.equal(await page.evaluate(()=>window.finishes.at(-1).transcriptId),null);
    await page.evaluate(()=>{window.delayInput=0;window.sendFn(3,true);});
    await page.getByRole('button',{name:'Stop recording',exact:true}).waitFor();
    await page.evaluate(()=>window.sendFn(3,false,true));
    await page.getByRole('button',{name:'Start recording',exact:true}).waitFor();
    assert.equal(await page.evaluate(()=>window.finishes.at(-1).transcriptId),null);
    assert.equal(await page.evaluate(()=>window.tracks.every(t=>t.readyState==='ended')),true);
    assert.equal(await page.locator('[data-transcript-id]').count(),1);
    await page.evaluate(()=>{window.rejectModel=true;window.sendFn(4,true);});
    await page.getByRole('alert').filter({hasText:'The private dictation runtime could not start.'}).waitFor();
    assert.equal(await page.evaluate(()=>window.finishes.at(-1).transcriptId),null);
    assert.deepEqual(errors,[]);
  }finally{await browser.close();await new Promise((resolve,reject)=>server.httpServer.close(e=>e?reject(e):resolve()));}
});
