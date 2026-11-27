import test from 'node:test';
import assert from 'node:assert/strict';
import {existsSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
import {preview} from 'vite';
import {chromium} from 'playwright-core';
const executable=process.env.CHROME_PATH||'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
test('recordings move through voice tools by card drag, WAV path and Send to, retaining source context',{skip:!existsSync(executable)},async()=>{
  const server=await preview({root:fileURLToPath(new URL('..',import.meta.url)),preview:{host:'127.0.0.1',port:0,open:false}});
  const browser=await chromium.launch({executablePath:executable,headless:true});
  let stage='setup';
  try{
    const page=await browser.newPage({viewport:{width:1300,height:1050}});const errors=[];page.on('pageerror',e=>errors.push(e.message));
    await page.addInitScript(()=>{
      if(window.self!==window.top)return;
      window.calls=[];window.imports=[];window.projects=[];window.cleanupDrafts=[];window.results=[];window.clipboard='';window.counter=0;window.jobs=[];
      Object.defineProperty(navigator,'clipboard',{value:{writeText:async value=>window.clipboard=value},configurable:true});
      const original={id:'source-1',name:'Creator recording.wav',text:'ጤና ዛሬ',original_text:'ሰላም ዛሬ',model:'amharic-ctc',model_version:'v0.2',created_at_ms:Date.now(),revision:2,words:[{id:0,word:'ሰላም',corrected:'ጤና',start:.1,end:.3},{id:1,word:'ዛሬ',corrected:'ዛሬ',start:.35,end:.6}],corrections:[{human_checked:true,original_text:'ሰላም',replacement:'ጤና',audio_start:.1,audio_end:.3}]};
      window.saved=[original];window.original=original;
      const wav=()=>{const b=new Uint8Array(32044),v=new DataView(b.buffer);for(const [at,text] of [[0,'RIFF'],[8,'WAVE'],[12,'fmt '],[36,'data']])for(let i=0;i<text.length;i++)v.setUint8(at+i,text.charCodeAt(i));v.setUint32(4,b.length-8,true);v.setUint32(16,16,true);v.setUint16(20,1,true);v.setUint16(22,1,true);v.setUint32(24,16000,true);v.setUint32(28,32000,true);v.setUint16(32,2,true);v.setUint16(34,16,true);v.setUint32(40,32000,true);return Array.from(b);};
      const context=()=>({schemaVersion:1,source:{kind:'transcript',id:'source-1'},audioSha256:'fixture-hash',text:original.text,words:original.words.map(w=>({word:w.corrected,start:w.start,end:w.end})),timingsCurrent:true,modelVersion:'v0.2',transcript:original,transcriptAudioSha256:'fixture-hash',transcriptHistory:[],editLog:null,editHistory:[],cleanup:null,cleanupHistory:[],editedRegions:[],lineage:[]});
      const reply=packageValue=>{const meta=new TextEncoder().encode(JSON.stringify({name:packageValue.name,context:packageValue.context})),audio=new Uint8Array(packageValue.bytes),wire=new Uint8Array(4+meta.length+audio.length);new DataView(wire.buffer).setUint32(0,meta.length,true);wire.set(meta,4);wire.set(audio,4+meta.length);return wire.buffer;};
      window.__TAURI_INTERNALS__={invoke:async(command,args)=>{
        window.calls.push(command);
        if(command==='system_summary')return {os:'macos',arch:'aarch64',logicalCpus:8,totalMemoryBytes:48*1024**3};
        if(command==='get_asr_status')return {state:'ready',message:'Ready',modelVersion:'v0.2'};
        if(command==='get_fixture_progress')return {state:'idle',downloadedBytes:0,totalBytes:0};
        if(command==='get_active_dictation_session')return null;
        if(command==='list_transcripts')return [...window.saved];
        if(command==='list_transcription_recordings')return window.imports.filter(i=>!window.saved.some(t=>t.source_recording_id===i.id));
        if(command==='get_transcript'){const t=window.saved.find(t=>t.id===args.id);if(!t)throw 'Transcript unavailable';return t;}
        if(command==='get_transcript_audio')return {name:'Creator recording.wav',bytes:wav(),sourceMedia:window.saved.find(t=>t.id===args.id)?.sourceMedia};
        if(command==='copy_transcript_wav_path'){window.clipboard='/mock/transcripts/source-1/audio.wav';return;}
        if(command==='get_transcript_wav_path'||command==='get_recording_transfer_path')return '/mock/transcripts/source-1/audio.wav';
        if(command==='copy_recording_transfer_path')return;
        if(command==='resolve_recording_transfer'){
          const input=args.input;
          if(input.kind==='path'&&input.path.includes('missing'))throw 'This WAV path could not be opened. Check that the file still exists.';
          if(input.kind==='file')return reply({name:input.name,bytes:input.bytes,context:input.context?{...input.context,text:input.text??input.context.text}:context()});
          if(input.kind==='cleanup')return reply({name:'Cleaned recording.wav',bytes:wav(),context:{...window.cleanupContext,source:{kind:'cleanup',id:input.id},audioSha256:'cleaned-fixture-hash',timingsCurrent:false,cleanup:{model_version:'cleanup-fixture'}}});
          if(input.kind==='editing'){const project=window.projects.find(p=>p.id===input.id),v=project.versions[project.head];return reply({name:project.name,bytes:wav(),context:{...project.sourceMedia,source:{kind:'editing',id:project.id},text:v.text,words:v.words,timingsCurrent:!!v.words.length,editedRegions:v.regions,editLog:project}});}
          return reply({name:original.name,bytes:wav(),context:context()});
        }
        if(command==='import_transcription_audio'){const item={id:`import-${++window.counter}`,name:args.name,sourceMedia:args.sourceMedia};window.imports.push(item);return item;}
        if(command==='adopt_recording_transfer'){const source=window.imports.find(i=>i.id===args.audioId),meta=source.sourceMedia;const item={...original,id:`copy-${++window.counter}`,source_recording_id:source.id,text:meta.text,model_version:meta.timingsCurrent?'v0.2':'imported',words:meta.timingsCurrent?meta.words.map((w,i)=>({id:i,word:w.word,corrected:w.word,start:w.start,end:w.end})):[],sourceMedia:meta};window.saved.push(item);return item;}
        if(command==='delete_transcript'){window.saved=window.saved.filter(t=>t.id!==args.id);return [];}
        if(command==='get_cleanup_status')return {state:'ready',message:'Ready'};
        if(command==='list_cleanup_results')return window.results;
        if(command==='list_cleanup_recordings')return window.cleanupDrafts;
        if(command==='get_active_cleanup_job')return null;
        if(command==='import_cleanup_audio'){const item={id:`cleanup-${++window.counter}`,name:args.name};window.cleanupContext=args.sourceMedia;window.cleanupDrafts.push(item);return item;}
        if(command==='get_cleanup_recording_audio')return {name:'Creator recording.wav',bytes:wav(),sourceMedia:window.cleanupContext};
        if(command==='start_cleanup'){const result={id:'result-1',name:'Cleaned recording.wav',sample_rate:16000,channels:1,frames:16000};window.results.push(result);return {id:'cleanup-job',state:'completed',stage:'Saved',resultId:result.id};}
        if(command==='get_cleanup_audio')return {name:'Cleaned recording.wav',bytes:wav(),sourceMedia:args.kind==='original'?window.cleanupContext:undefined};
        if(command==='get_voice_edit_status')return {selection:{available:false,opt_in_required:false,slow:false,reason:'Voice editing is unavailable on this fixture hardware.',tier:'apple-arm',download_bytes:0,licenses:[]},consent:true,optIn:false,ready:false,runtimePresent:false,busy:false,stage:'',loaded:false};
        if(command==='list_voice_edit_projects')return window.projects;
        if(command==='import_voice_edit_project'){const project={id:`edit-${++window.counter}`,name:args.name,sourceMedia:args.sourceMedia,created:Date.now()/1000,revision:0,head:0,duration:1,channels:1,versions:[{audio:'original.wav',text:args.text,words:args.words,regions:args.sourceMedia?.editedRegions??[],kind:'import',parent:null}]};window.projects.push(project);return project;}
        if(command==='get_voice_edit_audio')return {name:'original.wav',bytes:wav()};
        if(command==='start_transcription'){const result={...original,id:'fresh-timings',source_recording_id:args.audioId,model_version:'v0.2',created_at_ms:Date.now(),corrections:[]};window.saved.push(result);return {id:'fresh-job',state:'completed',stage:'Saved',transcriptId:result.id};}
        if(command==='voice_edit_action')throw Error('Transferring a recording must not run inference');
        throw Error('Outside this test');
      }};
    });
    const nav=label=>page.locator(`button.nav-item[aria-label="${label}"]`);
    await page.goto(server.resolvedUrls.local[0]);await nav('Dictation').click();
    const source=page.locator('[data-transcript-id="source-1"]');await source.waitFor();
    stage='copy path into speech reference';
    await source.getByRole('button',{name:'Copy WAV path for recording 1',exact:true}).click();
    await nav('Text to Speech').click();
    await page.getByRole('button',{name:'Manage voices',exact:true}).click();
    await page.locator('.recording-input summary').click();
    await page.getByLabel('Local audio path',{exact:true}).fill(await page.evaluate(()=>window.clipboard));
    await page.getByRole('button',{name:'Use recording',exact:true}).click();
    await page.getByLabel('Exact words in the reference WAV',{exact:true}).waitFor();
    assert.equal(await page.getByLabel('Exact words in the reference WAV',{exact:true}).inputValue(),'ጤና ዛሬ');
    assert.notEqual(await page.getByLabel('Script',{exact:true}).inputValue(),'ጤና ዛሬ','reference transcript must not replace the synthesis script');
    await page.getByLabel('Save a project voice profile',{exact:true}).fill('Creator voice');
    await page.getByRole('button',{name:'Save reference profile',exact:true}).click();
    await page.getByRole('status').filter({hasText:'Reference profile saved'}).waitFor();
    stage='invalid path retains existing reference';
    await page.getByLabel('Local audio path',{exact:true}).fill('/missing/recording.wav');await page.getByRole('button',{name:'Use recording',exact:true}).click();
    await page.getByRole('alert').filter({hasText:'This WAV path could not be opened.'}).waitFor();
    assert.equal(await page.getByLabel('Exact words in the reference WAV',{exact:true}).inputValue(),'ጤና ዛሬ');
    stage='drag transcript card into editing';
    await nav('Transcription').click();await source.waitFor();
    await source.getByRole('button',{name:'Drag recording 1 to another voice tool',exact:true}).dragTo(nav('Voice Editing'));
    await page.getByRole('region',{name:'Recording transcript',exact:true}).waitFor();
    assert.deepEqual(await page.evaluate(()=>window.projects[0].versions[0].words),[{word:'ጤና',start:.1,end:.3},{word:'ዛሬ',start:.35,end:.6}]);
    assert.equal(await page.evaluate(()=>window.projects[0].sourceMedia.transcript.corrections[0].human_checked),true);
    stage='send edited recording into cleanup';
    await page.getByLabel('Send edited recording to another tool',{exact:true}).click();
    await page.locator('.recording-actions .send-options').getByRole('button',{name:'Voice Cleanup',exact:true}).click();
    await page.getByRole('button',{name:'Clean up voice',exact:true}).waitFor();
    await page.waitForFunction(()=>!!window.cleanupContext);
    assert.equal(await page.evaluate(()=>window.cleanupContext.text),'ጤና ዛሬ');
    await page.getByRole('button',{name:'Clean up voice',exact:true}).click();
    await page.getByRole('button',{name:'Export cleaned WAV',exact:true}).waitFor();
    stage='send cleaned recording to dictation history';
    await page.getByLabel('Send cleaned recording to another tool',{exact:true}).click();
    await page.locator('.recording-actions .send-options').getByRole('button',{name:'Dictation',exact:true}).click();
    await page.locator('[data-transcript-id^="copy-"]').waitFor();
    assert.equal(await page.evaluate(()=>window.imports.at(-1).sourceMedia.timingsCurrent),false);
    assert.equal(await page.evaluate(()=>window.imports.at(-1).sourceMedia.transcript.model_version),'v0.2');
    assert.equal(await page.evaluate(()=>window.saved.at(-1).text),'ጤና ዛሬ');
    assert.equal(await page.evaluate(()=>window.calls.includes('start_transcription')),false);
    stage='fresh word timings require an explicit user action';
    await page.locator('[data-transcript-id^="copy-"]').getByRole('button',{name:'Get fresh word timings',exact:true}).click();
    await page.locator('[data-transcript-id="fresh-timings"]').waitFor();
    assert.equal(await page.locator('[data-transcript-id^="copy-"]').count(),1,'keep the copied transcript alongside the fresh recognition');
    stage='shared history navigation does not duplicate cards';
    const before=await page.evaluate(()=>window.imports.length);
    await source.getByLabel('Send recording 1 to another tool',{exact:true}).click();
    await source.locator('.send-options').getByRole('button',{name:'Transcription',exact:true}).click();
    await page.getByRole('region',{name:'Transcription workspace',exact:true}).waitFor();
    await page.waitForTimeout(100);assert.equal(await page.evaluate(()=>window.imports.length),before);
    stage='saved voice profile retains context after reload';
    await page.reload();await nav('Text to Speech').click();
    const choices=page.getByRole('combobox',{name:'Choose a voice',exact:true});await choices.locator('option').filter({hasText:'Creator voice'}).waitFor({state:'attached'});
    const value=await choices.locator('option').filter({hasText:'Creator voice'}).getAttribute('value');await choices.selectOption(value);
    await page.getByRole('button',{name:'Manage voices',exact:true}).click();
    assert.equal(await page.getByLabel('Exact words in the reference WAV',{exact:true}).inputValue(),'ጤና ዛሬ');
    stage='drag an independent reference file into cleanup';
    await page.getByRole('button',{name:'Drag reference recording to another voice tool',exact:true}).dragTo(nav('Voice Cleanup'));
    await page.waitForFunction(()=>window.cleanupDrafts.length===1);
    assert.equal(await page.evaluate(()=>window.cleanupContext.transcript.corrections[0].replacement),'ጤና');
    assert.equal(await page.evaluate(()=>window.calls.includes('start_transcription')),false);
    assert.equal(await page.evaluate(()=>window.calls.includes('voice_edit_action')),false);
    assert.deepEqual(errors,[]);
    await page.screenshot({path:'/tmp/ethiopic-recording-transfer-cleanup.png'});
  }catch(error){console.error('Transfer test stage:',stage);throw error;}
  finally{await browser.close();await new Promise((resolve,reject)=>server.httpServer.close(e=>e?reject(e):resolve()));}
});
