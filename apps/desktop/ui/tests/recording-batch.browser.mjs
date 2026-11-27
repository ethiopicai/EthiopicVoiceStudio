import test from 'node:test';
import assert from 'node:assert/strict';
import {existsSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
import {preview} from 'vite';
import {chromium} from 'playwright-core';
const executable=process.env.CHROME_PATH||'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
test('both pages support card selection, select-all, confirmed batch delete and partial failure retry',{skip:!existsSync(executable)},async()=>{
 const server=await preview({root:fileURLToPath(new URL('..',import.meta.url)),preview:{host:'127.0.0.1',port:0,open:false}});
 const browser=await chromium.launch({executablePath:executable,headless:true});
 try{
  for(const view of ['Transcription','Dictation']){
   const page=await browser.newPage();
   await page.addInitScript(()=>{
    window.saved=['first','second','third'].map((id,index)=>({id,name:`${id}.wav`,text:'Saved words',words:[],corrections:[],revision:1,model_version:'fixture',created_at_ms:index+1}));window.drafts=[{id:'draft',name:'pending.wav'}];window.requests=[];window.failRequest=false;window.failId='';
    window.__TAURI_INTERNALS__={invoke:async(command,args)=>{
     if(command==='get_asr_status')return {state:'ready',message:'Ready'};
     if(command==='get_fixture_progress')return {state:'idle',downloadedBytes:0,totalBytes:0};
     if(command==='list_transcripts')return window.saved;
     if(command==='list_transcription_recordings')return window.drafts;
     if(command.startsWith('get_active_'))return null;
     if(command==='get_transcript')return window.saved.find(t=>t.id===args.id);
     if(command==='get_transcript_audio')throw Error('No playback fixture needed');
     if(command==='delete_transcripts'){
      window.requests.push(args.items);if(window.failRequest)throw Error('Revision conflict');
      const result={deleted:[],failed:[],removedRecordingIds:[]};
      for(const item of args.items){
       if(item.id===window.failId){result.failed.push({...item,message:'Could not delete'});continue;}
       result.deleted.push({kind:item.kind,id:item.id});
       if(item.kind==='transcript')window.saved=window.saved.filter(t=>t.id!==item.id);
       else{window.drafts=window.drafts.filter(t=>t.id!==item.id);result.removedRecordingIds.push(item.id);}
      }
      return result;
     }
     throw Error('Not part of batch test');
    }};
   });
   await page.goto(server.resolvedUrls.local[0]);await page.locator(`button.nav-item[aria-label="${view}"]`).click();
   const first=page.locator('[data-transcript-id="first"]'),second=page.locator('[data-transcript-id="second"]'),third=page.locator('[data-transcript-id="third"]');await first.waitFor();
   const before=await first.locator('.transcript-card-heading').getAttribute('aria-expanded');
   await first.getByRole('checkbox',{name:/^Select recording/}).check();await second.getByRole('checkbox',{name:/^Select recording/}).check();
   assert.equal(await first.locator('.transcript-card-heading').getAttribute('aria-expanded'),before);
   const all=page.getByRole('checkbox',{name:'Select all recordings',exact:true});assert.equal(await all.evaluate(input=>input.indeterminate),true);
   const dialog=page.getByRole('dialog').filter({has:page.getByRole('heading',{name:'Delete 2 recordings?',exact:true})});
   await page.getByRole('button',{name:'Delete selected',exact:true}).click();await dialog.waitFor();await dialog.getByRole('button',{name:'Keep it',exact:true}).click();
   assert.equal(await page.evaluate(()=>window.requests.length),0);
   await page.evaluate(()=>window.failRequest=true);await page.getByRole('button',{name:'Delete selected',exact:true}).click();await dialog.getByRole('button',{name:'Delete from app',exact:true}).click();
   await page.getByRole('alert').waitFor();assert.equal(await first.count(),1);await page.getByRole('dialog').filter({visible:true}).getByRole('button',{name:'Keep it',exact:true}).click();
   await page.evaluate(()=>{window.failRequest=false;window.failId='second';});await page.getByRole('button',{name:'Delete selected',exact:true}).click();await dialog.getByRole('button',{name:'Delete from app',exact:true}).click();
   await first.waitFor({state:'detached'});await page.getByRole('alert').waitFor();assert.equal(await second.getByRole('checkbox',{name:/^Select recording/}).isChecked(),true);assert.equal(await third.getByRole('checkbox',{name:/^Select recording/}).isChecked(),false);
   await page.getByRole('dialog').filter({visible:true}).getByRole('button',{name:'Keep it',exact:true}).click();await page.evaluate(()=>window.failId='');
   await page.getByRole('button',{name:'Delete selected',exact:true}).click();await page.getByRole('dialog').filter({visible:true}).getByRole('button',{name:'Delete from app',exact:true}).click();await second.waitFor({state:'detached'});
   assert.equal(await third.count(),1);await all.check();await page.getByRole('button',{name:'Clear',exact:true}).click();assert.equal(await page.getByRole('button',{name:'Delete selected',exact:true}).count(),0);
   await all.check();await page.getByRole('button',{name:'Delete selected',exact:true}).click();await page.getByRole('dialog').filter({visible:true}).getByRole('button',{name:'Delete from app',exact:true}).click();await third.waitFor({state:'detached'});
   assert.equal(await page.locator('[data-recording-id="draft"]').count(),0);
   assert.equal(await all.isDisabled(),true);
   const requests=await page.evaluate(()=>window.requests);assert.deepEqual(requests[0].map(item=>item.id).sort(),['first','second']);assert.equal(requests.flat().some(item=>'name' in item),false);
   await page.close();
  }
 }finally{await browser.close();await new Promise(r=>server.httpServer.close(r));}
});
