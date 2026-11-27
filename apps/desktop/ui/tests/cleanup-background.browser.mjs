import test from 'node:test';
import assert from 'node:assert/strict';
import {existsSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
import {preview} from 'vite';
import {chromium} from 'playwright-core';
const executable=process.env.CHROME_PATH||'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';

test('Cleanup restores its recording and visible progress across navigation, retries playback, and opens background results',{skip:!existsSync(executable)},async()=>{
  const server=await preview({root:fileURLToPath(new URL('..',import.meta.url)),preview:{host:'127.0.0.1',port:0,open:false}});
  const browser=await chromium.launch({executablePath:executable,headless:true});
  try{
    const page=await browser.newPage({viewport:{width:1200,height:1000},colorScheme:'dark',reducedMotion:'reduce'});
    const errors=[];page.on('pageerror',error=>errors.push(error.message));
    await page.addInitScript(()=>{
      const b=new Uint8Array(32044),v=new DataView(b.buffer);
      for(const [at,text] of [[0,'RIFF'],[8,'WAVE'],[12,'fmt '],[36,'data']])for(let i=0;i<text.length;i++)v.setUint8(at+i,text.charCodeAt(i));
      v.setUint32(4,b.length-8,true);v.setUint32(16,16,true);v.setUint16(20,1,true);v.setUint16(22,1,true);v.setUint32(24,16000,true);v.setUint32(28,32000,true);v.setUint16(32,2,true);v.setUint16(34,16,true);v.setUint32(40,32000,true);
      window.wav=Array.from(b);window.cleanupCalls=[];window.sourceReads=0;window.failAudio=false;window.failPoll=false;
      window.cleanupJob=new URL(location.href).searchParams.has('active')?{id:'job-1',audioId:'source-1',name:'Podcast.wav',state:'running',stage:'Cleaning audio · 23%',progress:23}:null;
      window.__TAURI_INTERNALS__={invoke:async(command,args)=>{
        window.cleanupCalls.push(command);
        if(command==='system_summary')return {os:'macos',arch:'aarch64',logicalCpus:8,totalMemoryBytes:16*1024**3};
        if(command==='get_asr_status'||command==='get_cleanup_status')return {state:'ready',message:'Ready'};
        if(command==='get_active_dictation_session')return null;
        if(command==='list_cleanup_recordings')return [{id:'source-1',name:'Podcast.wav'}];
        if(command==='list_cleanup_results')return window.cleanupJob?.state==='completed'?[{id:'result-1',name:'Podcast.wav',sample_rate:16000,channels:1,frames:16000}]:[];
        if(command==='get_active_cleanup_job')return window.cleanupJob?.state==='running'?structuredClone(window.cleanupJob):null;
        if(command==='get_cleanup_job'){if(window.failPoll)throw 'Progress unavailable';return structuredClone(window.cleanupJob);}
        if(command==='get_cleanup_recording_audio'){window.sourceReads++;if(window.failAudio)throw 'Read unavailable';return {name:'Podcast.wav',bytes:window.wav};}
        if(command==='get_cleanup_audio')return {name:'Podcast.wav',bytes:window.wav};
        if(command==='start_cleanup'){window.cleanupJob={id:'job-1',audioId:args.audioId,name:'Podcast.wav',state:'running',stage:'Loading Voice Cleanup',progress:null};return structuredClone(window.cleanupJob);}
        if(command==='cancel_cleanup'){window.cleanupJob={...window.cleanupJob,state:'cancelled',stage:'Cancelled. Your recording is saved.'};return;}
        throw Error('Outside this test');
      }};
    });
    const nav=label=>page.locator(`button.nav-item[aria-label="${label}"]`);
    const original=page.getByRole('region',{name:'Original recording',exact:true});
    const bar=page.getByRole('progressbar',{name:'Voice cleanup in progress',exact:true});
    const play=original.getByRole('button',{name:'Play Original: Podcast.wav',exact:true});
    const transfer=page.getByRole('region',{name:'Use a recording from another tool',exact:true});
    const checkLocked=async()=>{
      assert.equal(await page.getByLabel('Choose recording for Voice Cleanup',{exact:true}).isDisabled(),true);
      assert.equal(await original.getByRole('button',{name:'Replace',exact:true}).isDisabled(),true);
      assert.equal(await page.locator('.cleanup-history-item').evaluateAll(items=>items.length>0&&items.every(item=>item.disabled)),true);
      assert.equal(await page.getByRole('button',{name:'Refresh cleanup history',exact:true}).isDisabled(),true);
      assert.equal(await transfer.getByLabel('Local audio path',{exact:true}).isDisabled(),true);
      assert.equal(await transfer.getByRole('button',{name:'Use recording',exact:true,includeHidden:true}).isDisabled(),true);
      assert.equal(await transfer.locator('summary').getAttribute('aria-disabled'),'true');
      await transfer.locator('summary').evaluate(element=>element.click());
      assert.equal(await transfer.locator('details').evaluate(element=>element.open),false,'disabled import disclosure stays closed');
      const send=page.getByLabel('Send cleanup recording to another tool',{exact:true});
      assert.equal(await send.getAttribute('aria-disabled'),'true');
      await send.evaluate(element=>element.click());
      assert.equal(await send.evaluate(element=>element.parentElement.open),false,'disabled Send to stays closed');
      assert.equal(await play.isEnabled(),true);
      assert.equal(await page.getByRole('button',{name:'Cancel cleanup',exact:true}).isEnabled(),true);
    };
    await page.goto(server.resolvedUrls.local[0]);await nav('Voice Cleanup').click();
    await page.getByRole('button',{name:'Open original recording: Podcast.wav',exact:true}).click();
    await play.waitFor();
    await transfer.locator('summary').click();
    await transfer.getByLabel('Local audio path',{exact:true}).fill('/another-recording.wav');
    await page.getByRole('button',{name:'Clean up voice',exact:true}).click();
    await bar.waitFor();assert.equal(await bar.getAttribute('aria-valuenow'),null);
    await checkLocked();
    for(const target of [transfer,page.getByRole('region',{name:'Voice Cleanup workspace',exact:true})]){
      const dataTransfer=await page.evaluateHandle(()=>{const data=new DataTransfer();data.setData('application/x-ethiopic-studio-recording',JSON.stringify({version:1,input:{kind:'transcript',id:'another-recording'}}));return data;});
      await target.dispatchEvent('drop',{dataTransfer});await dataTransfer.dispose();
    }
    assert.equal(await page.evaluate(()=>window.cleanupCalls.some(command=>command==='resolve_recording_transfer'||command==='import_cleanup_audio')),false,'locked drops never resolve or import another recording');
    await play.click();await original.getByRole('button',{name:'Pause Original: Podcast.wav',exact:true}).click();
    assert.equal(await bar.locator('span').evaluate(element=>getComputedStyle(element).animationName),'none','reduced motion still shows a static fill');
    assert.ok(await bar.locator('span').evaluate(element=>element.getBoundingClientRect().width)>0);
    await page.evaluate(()=>{window.cleanupJob.progress=37;window.cleanupJob.stage='Cleaning audio · 37%';});
    await page.waitForFunction(()=>document.querySelector('[role="progressbar"]')?.getAttribute('aria-valuenow')==='37');
    const ratio=await bar.evaluate(element=>element.firstElementChild.getBoundingClientRect().width/element.getBoundingClientRect().width);
    assert.ok(Math.abs(ratio-.37)<.02,`visible bar fill ${ratio} should match 37%`);
    await nav('Text to Speech').click();
    await nav('Voice Cleanup').click();await play.waitFor();
    await checkLocked();
    assert.equal(await bar.getAttribute('aria-valuenow'),'37');
    assert.ok(await page.evaluate(()=>window.sourceReads)>=2,'returning opens the job source waveform');
    assert.equal(await page.getByRole('button',{name:'Open original recording: Podcast.wav',exact:true}).getAttribute('aria-current'),'true');
    assert.equal(await original.getByRole('heading',{name:'Start with a recording',exact:true}).count(),0);
    await page.evaluate(()=>{window.cleanupJob.progress=75;window.cleanupJob.stage='Cleaning audio · 75%';});
    await page.waitForFunction(()=>document.querySelector('[role="progressbar"]')?.getAttribute('aria-valuenow')==='75');
    await page.screenshot({path:'/tmp/ethiopic-cleanup-background.png'});
    await nav('Text to Speech').click();await page.evaluate(()=>window.failAudio=true);
    await nav('Voice Cleanup').click();
    await original.getByRole('button',{name:'Retry opening recording',exact:true}).waitFor();
    assert.match(await original.innerText(),/Podcast\.wav/);
    assert.equal(await bar.getAttribute('aria-valuenow'),'75');
    await page.evaluate(()=>window.failAudio=false);
    await original.getByRole('button',{name:'Retry opening recording',exact:true}).click();await play.waitFor();
    await nav('Text to Speech').click();
    await page.evaluate(()=>{window.cleanupJob={...window.cleanupJob,state:'completed',stage:'Cleaned audio saved',progress:100,resultId:'result-1'};});
    await nav('Voice Cleanup').click();
    await page.getByRole('button',{name:'Export cleaned WAV',exact:true}).waitFor();
    assert.equal(await page.locator('.cleanup-panels .waveform-player').count(),2);
    assert.equal(await page.getByLabel('Choose recording for Voice Cleanup',{exact:true}).isEnabled(),true,'completion unlocks file import');
    assert.equal(await original.getByRole('button',{name:'Replace',exact:true}).isEnabled(),true);
    assert.equal(await page.evaluate(()=>window.cleanupCalls.filter(c=>c==='start_cleanup').length),1,'navigation never starts another inference');
    await page.goto(`${server.resolvedUrls.local[0]}?active=1`);await nav('Voice Cleanup').click();await play.waitFor();
    assert.equal(await bar.getAttribute('aria-valuenow'),'23','an active native job restores without an in-memory job ID');
    await checkLocked();
    await page.getByRole('button',{name:'Cancel cleanup',exact:true}).click();
    await bar.waitFor({state:'detached'});await play.waitFor();
    assert.equal(await page.getByRole('button',{name:'Clean up voice',exact:true}).isEnabled(),true);
    assert.equal(await page.getByLabel('Choose recording for Voice Cleanup',{exact:true}).isEnabled(),true);
    assert.equal(await original.getByRole('button',{name:'Replace',exact:true}).isEnabled(),true);
    await transfer.locator('summary').click();
    assert.equal(await transfer.getByLabel('Local audio path',{exact:true}).isEnabled(),true,'cancelling unlocks imports');
    assert.deepEqual(errors,[]);
  }finally{await browser.close();await new Promise((resolve,reject)=>server.httpServer.close(error=>error?reject(error):resolve()));}
});
