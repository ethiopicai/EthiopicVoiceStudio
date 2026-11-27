import test from 'node:test';
import assert from 'node:assert/strict';
import {existsSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
import {preview} from 'vite';
import {chromium} from 'playwright-core';
const executable=process.env.CHROME_PATH||'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';

test('history deletion requires confirmation, clears cards/imports, and handles failure without hiding saved work',{skip:!existsSync(executable)},async()=>{
  const server=await preview({root:fileURLToPath(new URL('..',import.meta.url)),preview:{host:'127.0.0.1',port:0,open:false}});
  const browser=await chromium.launch({executablePath:executable,headless:true});
  try{
    const page=await browser.newPage();
    await page.addInitScript(()=>{
      const make=(id,name)=>({id,name,model_version:'test-v1',revision:2,source_recording_id:`source-${id}`,text:'Test transcript',words:[],corrections:[]});
      window.saved=[make('first','first.wav'),make('second','second.wav')];
      window.imports=[{id:'source-first',name:'first.wav'},{id:'source-second',name:'second.wav'},{id:'pending',name:'pending.wav'},{id:'discard',name:'discard.wav'}];
      window.deletions=[];window.failDelete=false;
      window.__TAURI_INTERNALS__={invoke:async(command,args)=>{
        if(command==='get_asr_status')return {state:'ready',message:'Ready'};
        if(command==='list_transcripts')return window.saved;
        if(command==='list_transcription_recordings')return window.imports.filter(item=>!window.saved.some(t=>t.source_recording_id===item.id));
        if(command==='get_transcript')return window.saved.find(t=>t.id===args.id);
        if(command==='import_transcription_audio'){const item={id:'chosen-file',name:args.name};window.imports.push(item);return item;}
        if(command==='get_transcript_audio')throw Error('No test audio');
        if(command==='get_transcription_recording_audio')return {name:window.imports.find(i=>i.id===args.audioId).name,bytes:[]};
        if(command==='start_transcription'){
          const saved={...make('pending-result','pending.wav'),source_recording_id:args.audioId};window.saved.push(saved);
          return {id:'recognition',state:'completed',stage:'Transcript saved',transcriptId:saved.id};
        }

        if(command==='get_active_transcription_job')return null;
        if(command==='get_fixture_progress')return {state:'idle',downloadedBytes:0,totalBytes:0};
        if(command==='delete_transcript'){
          if(window.failDelete)throw Error('Storage unavailable');
          window.deletions.push(args);window.saved=window.saved.filter(t=>t.id!==args.id);
          const removed=`source-${args.id}`;window.imports=window.imports.filter(t=>t.id!==removed);return [removed];
        }
        if(command==='delete_transcription_recording'){
          window.deletions.push(args);window.imports=window.imports.filter(t=>t.id!==args.audioId);return null;
        }
        throw Error('Not part of this test');
      }};
    });
    await page.goto(server.resolvedUrls.local[0]);
    await page.locator('button.nav-item[aria-label="Transcription"]').click();
    const first=page.locator('#transcript-first');
    await first.waitFor();
    const divider=page.getByRole('separator',{name:'Resize transcription history'});
    const initial=Number(await divider.getAttribute('aria-valuenow'));
    const handle=await divider.boundingBox();
    await page.mouse.move(handle.x+handle.width/2,handle.y+100);
    await page.mouse.down();await page.mouse.move(handle.x+104,handle.y+100);await page.mouse.up();
    const expanded=Number(await divider.getAttribute('aria-valuenow'));
    assert.ok(expanded>=initial+90);
    await divider.focus();await page.keyboard.press('ArrowLeft');
    const resized=Number(await divider.getAttribute('aria-valuenow'));
    assert.equal(resized,expanded-20);
    await page.getByRole('button',{name:'Hide transcription history',exact:true}).click();
    await page.getByRole('button',{name:'Show history',exact:true}).click();
    assert.equal(Number(await divider.getAttribute('aria-valuenow')),resized);
    await page.locator('button.nav-item[aria-label="Home"]').click();
    await page.locator('button.nav-item[aria-label="Transcription"]').click();
    await first.waitFor();
    assert.equal(Number(await divider.getAttribute('aria-valuenow')),resized);

    await page.locator('.history-list .history-item').filter({hasText:'first.wav'}).click({button:'right'});
    await page.getByRole('menuitem',{name:'Delete transcription',exact:true}).click();
    const dialog=page.getByRole('dialog');
    await dialog.waitFor();
    assert.match(await dialog.textContent(),/original files.*stay unchanged/);
    assert.equal(await page.evaluate(()=>document.activeElement.textContent),'Keep it');
    await page.keyboard.press('Escape');
    assert.equal(await page.evaluate(()=>window.deletions.length),0);
    assert.equal(await first.count(),1);
    await page.keyboard.press('Shift+F10');
    await page.getByRole('menuitem',{name:'Delete transcription',exact:true}).waitFor();
    await page.keyboard.press('Escape');
    assert.equal(await page.getByRole('menu',{name:'Transcription actions'}).count(),0);
    await first.getByRole('button',{name:/Delete recording/}).click();
    await dialog.getByRole('button',{name:'Delete from app',exact:true}).click();
    await first.waitFor({state:'detached'});
    assert.deepEqual(await page.evaluate(()=>window.deletions),[{id:'first',revision:2}]);
    assert.equal(await page.locator('.history-list .history-item').filter({hasText:'first.wav'}).count(),0);
    assert.equal(await page.getByRole('heading',{name:'Imported recordings',exact:true}).count(),0);
    assert.equal(await page.locator('#transcript-second').count(),1);
    const pending=page.locator('[data-recording-id="pending"]');
    await pending.locator('.pending-card-heading').click();
    await pending.getByRole('button',{name:'Transcribe recording',exact:true}).click();
    await pending.waitFor({state:'detached'});
    await page.locator('#transcript-pending-result').waitFor();
    assert.equal(await page.locator('.history-list .history-item').filter({hasText:'pending.wav'}).count(),1);
    await page.evaluate(()=>window.failDelete=true);
    await page.locator('#transcript-second').getByRole('button',{name:/Delete recording/}).click();
    await dialog.getByRole('button',{name:'Delete from app',exact:true}).click();
    await page.getByRole('alert').waitFor();
    await page.getByRole('dialog').getByRole('button',{name:'Keep it',exact:true}).click();
    assert.equal(await page.locator('#transcript-second').count(),1);
    await page.locator('.history-list .history-item').filter({hasText:'discard.wav'}).click({button:'right'});
    await page.getByRole('menuitem',{name:'Delete transcription',exact:true}).click();
    await dialog.getByRole('button',{name:'Delete from app',exact:true}).click();
    await page.locator('.history-list .history-item').filter({hasText:'discard.wav'}).waitFor({state:'detached'});
    assert.equal(await page.locator('#transcript-second').count(),1);
    const wav=Buffer.alloc(3244);wav.write('RIFF',0);wav.writeUInt32LE(3236,4);wav.write('WAVE',8);wav.write('fmt ',12);wav.writeUInt32LE(16,16);wav.writeUInt16LE(1,20);wav.writeUInt16LE(1,22);wav.writeUInt32LE(16000,24);wav.writeUInt32LE(32000,28);wav.writeUInt16LE(2,32);wav.writeUInt16LE(16,34);wav.write('data',36);wav.writeUInt32LE(3200,40);
    await page.getByLabel('Choose audio recording',{exact:true}).setInputFiles({name:'chosen.wav',mimeType:'audio/wav',buffer:wav});
    await page.locator('[data-recording-id="chosen-file"]').waitFor();
    assert.equal(await page.locator('.history-list .history-item').filter({hasText:'chosen.wav'}).count(),1);
  }finally{
    await browser.close();
    await new Promise((resolve,reject)=>server.httpServer.close(error=>error?reject(error):resolve()));
  }
});
