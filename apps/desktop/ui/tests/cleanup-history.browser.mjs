import test from 'node:test';
import assert from 'node:assert/strict';
import {existsSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
import {preview} from 'vite';
import {chromium} from 'playwright-core';
const executable=process.env.CHROME_PATH||'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';

test('Cleanup history visibly lists saved originals/results, reopens each, resizes and remembers collapse without hiding saved work on refresh failure',{skip:!existsSync(executable)},async()=>{
  const server=await preview({root:fileURLToPath(new URL('..',import.meta.url)),preview:{host:'127.0.0.1',port:0,open:false}});
  const browser=await chromium.launch({executablePath:executable,headless:true});
  try{
    const page=await browser.newPage({viewport:{width:1300,height:950},colorScheme:'dark'});
    const errors=[];page.on('pageerror',error=>errors.push(error.message));
    await page.addInitScript(()=>{
      const bytes=new Uint8Array(32044),v=new DataView(bytes.buffer);
      for(const [at,text] of [[0,'RIFF'],[8,'WAVE'],[12,'fmt '],[36,'data']])for(let i=0;i<text.length;i++)v.setUint8(at+i,text.charCodeAt(i));
      v.setUint32(4,bytes.length-8,true);v.setUint32(16,16,true);v.setUint16(20,1,true);v.setUint16(22,1,true);v.setUint32(24,16000,true);v.setUint32(28,32000,true);v.setUint16(32,2,true);v.setUint16(34,16,true);v.setUint32(40,32000,true);
      window.wav=Array.from(bytes);window.historyCalls=[];window.failHistory=false;
      window.cleanupSources=Array.from({length:14},(_,i)=>({id:String(i+1),name:i===0?'Podcast.wav':i===13?'አማርኛ-ፖድካስት-በጣም-ረጅም-ስም-ያለው-የድምጽ-ቀረጻ-ለሙከራ.wav':`Podcast ${i+1}.wav`}));
      window.cleanupResults=window.cleanupSources.map(item=>({...item,sample_rate:16000,channels:1,frames:16000}));
      window.__TAURI_INTERNALS__={invoke:async(command,args)=>{
        window.historyCalls.push({command,args});
        if(command==='system_summary')return {os:'macos',arch:'aarch64',logicalCpus:8,totalMemoryBytes:16*1024**3};
        if(command==='get_asr_status'||command==='get_cleanup_status')return {state:'ready',message:'Ready'};
        if(command==='get_active_dictation_session'||command==='get_active_cleanup_job')return null;
        if(command==='list_cleanup_recordings'){if(window.failHistory)throw 'History temporarily unavailable';return structuredClone(window.cleanupSources);}
        if(command==='list_cleanup_results'){if(window.failHistory)throw 'History temporarily unavailable';return structuredClone(window.cleanupResults);}
        if(command==='get_cleanup_recording_audio')return {name:window.cleanupSources.find(item=>item.id===args.audioId).name,bytes:window.wav};
        if(command==='get_cleanup_audio')return {name:window.cleanupResults.find(item=>item.id===args.id).name,bytes:window.wav};
        throw Error('Outside this test');
      }};
    });
    const openCleanup=()=>page.locator('button.nav-item[aria-label="Voice Cleanup"]').click();
    const history=page.getByRole('region',{name:'Cleanup history',exact:true});
    const rows=history.locator('.cleanup-history-item');
    const resizer=page.getByRole('separator',{name:'Resize cleanup history',exact:true});
    await page.goto(server.resolvedUrls.local[0]);await openCleanup();
    await page.waitForFunction(()=>document.querySelectorAll('.cleanup-history-item').length===28);
    assert.equal(await page.evaluate(()=>window.historyCalls.filter(call=>call.command==='get_cleanup_audio'||call.command==='get_cleanup_recording_audio').length),0,'listing history does not decode every recording');
    assert.equal(await history.locator('.cleanup-history-list').evaluate(element=>element.scrollHeight>element.parentElement.clientHeight),true,'history scrolls independently');
    const longName=rows.filter({hasText:'አማርኛ-ፖድካስት'}).first().locator('strong');
    assert.equal(await longName.evaluate(element=>element.scrollWidth<=element.clientWidth+1),true,'long filenames wrap instead of being clipped');
    await history.getByRole('button',{name:'Open original recording: Podcast.wav',exact:true}).click();
    await page.getByRole('button',{name:'Play Original: Podcast.wav',exact:true}).waitFor();
    assert.equal(await history.getByRole('button',{name:'Open original recording: Podcast.wav',exact:true}).getAttribute('aria-current'),'true');
    await history.getByRole('button',{name:'Open cleaned recording: Podcast.wav',exact:true}).click();
    await page.getByRole('button',{name:'Export cleaned WAV',exact:true}).waitFor();
    assert.equal(await page.locator('.cleanup-panels .waveform-player').count(),2);
    assert.equal(await history.getByRole('button',{name:'Open cleaned recording: Podcast.wav',exact:true}).getAttribute('aria-current'),'true','same IDs/names in each category remain distinct');
    assert.equal(await history.getByRole('button',{name:'Open original recording: Podcast.wav',exact:true}).getAttribute('aria-current'),null);
    const before=Number(await resizer.getAttribute('aria-valuenow'));await resizer.focus();await page.keyboard.press('ArrowRight');
    assert.equal(Number(await resizer.getAttribute('aria-valuenow')),before+20);
    const box=await resizer.boundingBox();await page.mouse.move(box.x+box.width/2,box.y+100);await page.mouse.down();await page.mouse.move(box.x+box.width/2+60,box.y+100);await page.mouse.up();
    const savedWidth=Number(await resizer.getAttribute('aria-valuenow'));assert.ok(savedWidth>before+20);
    await page.screenshot({path:'/tmp/ethiopic-cleanup-history.png'});
    await page.evaluate(()=>window.failHistory=true);
    await page.getByRole('button',{name:'Refresh cleanup history',exact:true}).click();
    await history.getByRole('alert').filter({hasText:'Some history could not be loaded.'}).waitFor();
    assert.equal(await rows.count(),28,'a failed refresh retains the last saved list');
    await page.getByRole('button',{name:'Hide cleanup history',exact:true}).click();
    await page.getByRole('button',{name:'Show cleanup history',exact:true}).waitFor();
    await page.reload();await openCleanup();
    assert.equal(await history.count(),0,'collapse preference persists after reopening the app');
    await page.getByRole('button',{name:'Show cleanup history',exact:true}).click();
    await page.waitForFunction(()=>document.querySelectorAll('.cleanup-history-item').length===28);
    assert.equal(Number(await resizer.getAttribute('aria-valuenow')),savedWidth,'history width persists');
    await page.setViewportSize({width:700,height:900});
    await page.waitForTimeout(50);
    assert.equal(await page.locator('.history-layout').evaluate(element=>element.scrollWidth<=element.clientWidth+1),true,'history fits a narrow window');
    await page.evaluate(()=>{window.cleanupSources=[];window.cleanupResults=[];});
    await page.getByRole('button',{name:'Refresh cleanup history',exact:true}).click();
    await history.getByText('No cleanup recordings yet. Import a recording to get started.',{exact:true}).waitFor();
    assert.equal(await page.evaluate(()=>window.historyCalls.some(call=>call.command==='start_cleanup')),false,'opening history never starts processing');
    assert.deepEqual(errors,[]);
  }finally{await browser.close();await new Promise((resolve,reject)=>server.httpServer.close(error=>error?reject(error):resolve()));}
});
