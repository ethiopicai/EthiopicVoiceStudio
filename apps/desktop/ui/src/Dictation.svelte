<script lang="ts">
  import { onMount } from 'svelte';
  import NoiseReductionControl from './NoiseReductionControl.svelte';
  import { invoke } from '@tauri-apps/api/core';
  import {recordingTitles,displayRecordingTitle,forgetRecordingTitles} from './recording-titles.js';
  import {recordingSelectionKey,toggleRecordingSelection} from './recording-selection.js';
  import type {RecordingDeleteItem,RecordingBatchDeleteResult} from './recording-selection-types';
  import { DictationResampler, DictationSendQueue, dictationWav } from './dictation-audio.js';
  import {preferences,microphoneConstraints} from './preferences.js';
  import {dictationStartupMessage} from './dictation-startup.js';
  import {fnGesture,clearFnGesture,type FnGesture} from './fn-dictation';
  import WaveformPlayer from './WaveformPlayer.svelte';
  import TranscriptCard from './TranscriptCard.svelte';
  import RecordingWorkspace from './RecordingWorkspace.svelte';
  import {scrollToWorkspaceCard} from './card-navigation.js';
  import RecordingDeleteDialog from './RecordingDeleteDialog.svelte';
  import DraftRecordingCard from './DraftRecordingCard.svelte';
  import RecordingInput from './RecordingInput.svelte';
  import {readRecordingDrop,requestRecordingTransfer} from './recording-transfer.js';
  import {importRecordingHistory} from './recording-transfer.js';
  import type {Transcript} from './transcript-types';
  import { tick } from 'svelte';
  let {onSetup,settingsOpen=$bindable(false),historyOpen=$bindable(true)}:{onSetup:()=>void;settingsOpen?:boolean;historyOpen?:boolean}=$props();
  let selection=$state<string[]>([]),batchDeleting=$state(false);
  function toggleChecked(id:string,kind:'transcript'|'recording',checked:boolean){selection=toggleRecordingSelection(selection,kind,id,checked);}
  type Word={word:string;start:number;end:number};
  type Session={id:string;state:string;stage:string;finalWords:Word[];partialWords:Word[];error?:string|null;transcriptId?:string|null;recordingId?:string|null;decodeSeconds?:number|null;inputSampleRate?:number};
  type Job={id:string;state:string;stage:string;error?:string|null;transcriptId?:string|null};
  type DeleteTarget={id:string;name:string;revision?:number;kind:'transcript'|'recording'};
  let recordings=$state<{id:string;name:string}[]>([]),selectedDraft=$state(''),draftPreview=$state(''),draftBusy=$state(false),retryingDraftId=$state('');
  const drafts=$derived(recordings.filter(item=>!history.some(t=>t.source_recording_id===item.id)));
  let history=$state<Transcript[]>([]),activeAudioId=$state(''),historyLoading=$state(true),liveText=$state<HTMLDivElement>();
  let deleteTarget=$state<DeleteTarget|null>(null),deleting=$state(false),deleteError=$state(''),notice=$state('');
  $effect(()=>{if(notice){const timer=setTimeout(()=>notice='',4000);return()=>clearTimeout(timer);}});
  function askDelete(transcript:Transcript){if(composing||deleting||batchDeleting)return;deleteError='';deleteTarget={...transcript,name:displayRecordingTitle($recordingTitles,'transcript',transcript.id,transcript.name),kind:'transcript'};}
  function closeDelete(){if(!deleting){deleteTarget=null;deleteError='';}}
  async function deleteRecording(){
    if(!deleteTarget||deleting)return;const target=deleteTarget;deleting=true;deleteError='';historyRequest++;
    try{
      const removed=target.kind==='transcript'?await invoke<string[]>('delete_transcript',{id:target.id,revision:target.revision}):(await invoke('delete_transcription_recording',{audioId:target.id}),[target.id]);
      forgetRecordingTitles([{kind:target.kind,id:target.id},...removed.map(id=>({kind:'recording',id}))]);
      if(!alive)return;history=history.filter(item=>target.kind!=='transcript'||item.id!==target.id);recordings=recordings.filter(item=>!removed.includes(item.id));
      if(activeAudioId===target.id)activeAudioId='';if(removed.includes(selectedDraft)){selectedDraft='';releaseDraft();}if(session?.transcriptId===target.id)session=null;if(job?.transcriptId===target.id)job=null;deleting=false;closeDelete();notice='Transcription deleted from this app. Your original file is unchanged.';
    }catch{if(alive){deleteError='Could not finish deleting. Close active voice tasks and try again. Your original files are unchanged.';await loadHistory();}}
    finally{if(alive)deleting=false;}
  }
  async function deleteBatch(items:RecordingDeleteItem[]):Promise<RecordingBatchDeleteResult>{
    const result=await invoke<RecordingBatchDeleteResult>('delete_transcripts',{items});
    forgetRecordingTitles([...result.deleted,...result.removedRecordingIds.map(id=>({kind:'recording',id}))]);
    if(!alive)return result;historyRequest++;
    const deleted=new Set(result.deleted.filter(item=>item.kind==='transcript').map(item=>item.id));
    const removed=new Set([...result.removedRecordingIds,...result.deleted.filter(item=>item.kind==='recording').map(item=>item.id)]);
    history=history.filter(item=>!deleted.has(item.id));recordings=recordings.filter(item=>!removed.has(item.id));
    if(deleted.has(activeAudioId))activeAudioId='';if(removed.has(selectedDraft)){selectedDraft='';releaseDraft();}
    if(session?.transcriptId&&deleted.has(session.transcriptId))session=null;
    if(job?.transcriptId&&deleted.has(job.transcriptId))job=null;
    return result;
  }
  let historyRequest=0;
  const desktop='__TAURI_INTERNALS__' in window;
  async function revealRecording(id:string){await tick();if(alive&&activeAudioId===id&&!composing)scrollToWorkspaceCard(`transcript-${id}`);}
  async function loadHistory(){
    const request=++historyRequest;let latest='';
    try{
      const [next,pending]=await Promise.all([invoke<Transcript[]>('list_transcripts'),invoke<{id:string;name:string}[]>('list_transcription_recordings')]);
      if(alive&&request===historyRequest){history=next.sort((a,b)=>(a.created_at_ms??0)-(b.created_at_ms??0));recordings=pending;if(!activeAudioId&&!selectedDraft&&!composing){latest=history.at(-1)?.id??'';activeAudioId=latest;}}
    }catch{if(alive)error='Could not load saved recordings. Refresh history and try again.';}
    finally{if(alive)historyLoading=false;}
    if(latest&&request===historyRequest)await revealRecording(latest);
  }
  function releaseDraft(){if(draftPreview)URL.revokeObjectURL(draftPreview);draftPreview='';}
  async function openDraft(id:string,toggle=true){if(composing||draftBusy||deleting||batchDeleting)return;if(selectedDraft===id){if(toggle){selectedDraft='';releaseDraft();}else{await tick();scrollToWorkspaceCard(`draft-${id}`);}return;}draftBusy=true;error='';try{const audio=await invoke<{name:string;bytes:number[]}>('get_transcription_recording_audio',{audioId:id});if(!alive)return;releaseDraft();draftPreview=URL.createObjectURL(new Blob([new Uint8Array(audio.bytes)],{type:'audio/wav'}));selectedDraft=id;activeAudioId='';await tick();scrollToWorkspaceCard(`draft-${id}`);}catch{if(alive)error='This recording could not be opened. Try again.';}finally{if(alive)draftBusy=false;}}
  async function receiveRecording(value:import('./recording-transfer.js').RecordingPackage,input?:import('./recording-transfer.js').RecordingInput){if(composing||deleting||batchDeleting||draftBusy)throw Error('Finish your dictation before importing another recording.');draftBusy=true;error='';try{const received=await importRecordingHistory(value,input);if(!alive)return;await loadHistory();if(received.transcript){activeAudioId=received.transcript.id;selectedDraft='';releaseDraft();}else if(received.imported){selectedDraft=received.imported.id;activeAudioId='';releaseDraft();draftPreview=URL.createObjectURL(new Blob([new Uint8Array(value.bytes)],{type:'audio/wav'}));}}finally{if(alive)draftBusy=false;}}
  async function retranscribe(transcript:Transcript){if(!transcript.source_recording_id||composing||deleting||batchDeleting||draftBusy)return;selectedDraft=transcript.source_recording_id;await retryDraft();}
  async function retryDraft(){if(!selectedDraft||composing||deleting||batchDeleting||status.state!=='ready')return;retryingDraftId=selectedDraft;busy=true;error='';session=null;try{job=await invoke<Job>('start_transcription',{audioId:selectedDraft});if(job.state==='completed')await complete(job.transcriptId);}catch{if(alive){busy=false;retryingDraftId='';error='Transcription could not start. Your recording is still saved.';}}}
  function selectHistory(id:string,kind:'transcript'|'recording'){if(kind==='recording')void openDraft(id,false);else if(!recording){selectedDraft='';releaseDraft();activeAudioId=id;void tick().then(()=>scrollToWorkspaceCard(`transcript-${id}`));}}
  function requestDelete(id:string,kind:'transcript'|'recording'){if(kind==='transcript'){const t=history.find(item=>item.id===id);if(t)askDelete(t);}else if(!composing&&!deleting){const draft=drafts.find(item=>item.id===id);if(draft){deleteError='';deleteTarget={...draft,kind};}}}
  function updated(value:Transcript){history=history.map(t=>t.id===value.id?value:t);}
  async function complete(id?:string|null){
    if(!id){busy=false;error='The saved transcript could not be opened. Your recording is kept below.';return;}
    const token=generation;++historyRequest;
    try{const transcript=await invoke<Transcript>('get_transcript',{id});if(!valid(token))return;history=[...history.filter(item=>item.id!==id),transcript].sort((a,b)=>(a.created_at_ms??0)-(b.created_at_ms??0));activeAudioId=id;recordings=recordings.filter(item=>item.id!==transcript.source_recording_id&&item.id!==retryingDraftId);retryingDraftId='';selectedDraft='';releaseDraft();saved=true;busy=false;stage='Saved';releaseUrl();chunks=[];await finishFn(id);await revealRecording(id);}
    catch{if(valid(token)){busy=false;error='Your transcript was saved, but could not be opened here. Refresh history to retry.';}}
  }
  let mode=$state('live'),status=$state({state:'checking',message:'Checking Amharic voice tools…'});
  let recording=$state(false),busy=$state(false),stage=$state(''),error=$state(''),seconds=$state(0),saved=$state(false),session=$state<Session|null>(null),job=$state<Job|null>(null),audioUrl=$state('');
  const composing=$derived(busy||recording);
  $effect(()=>{
    const target=composing&&!retryingDraftId?'dictation-live':audioUrl&&!saved&&!recording?'dictation-recovered':'';
    if(target)void tick().then(()=>{if(alive)scrollToWorkspaceCard(target);});
  });
  $effect(()=>{const words=session?.finalWords.length??0,tail=session?.partialWords.map(word=>word.word).join(' ')??'';if(words||tail)void tick().then(()=>{if(alive&&liveText)liveText.scrollTop=liveText.scrollHeight;});});
  let alive=true,generation=0,polling=false,stream:MediaStream|null=null,context:AudioContext|null=null,node:AudioWorkletNode|null=null,source:MediaStreamAudioSourceNode|null=null,mute:GainNode|null=null;
  let starting=false,startupPhase:import('./dictation-startup.js').StartupPhase='audio';
  let fnRequest=$state(0),fnHeld=false,stopAfterStartup=false;
  async function finishFn(transcriptId:string|null=null){
    const requestId=fnRequest;if(!requestId)return;fnRequest=0;fnHeld=false;clearFnGesture(requestId);
    try{const message=await invoke<string>('finish_fn_dictation',{requestId,transcriptId});if(alive&&message)notice=message;}
    catch{if(alive)notice='Your dictation is available in Studio. Copy the text from its card.';}
  }
  async function shortcut(event:FnGesture|null){
    if(!event)return;
    if(event.pressed){
      if(fnRequest===event.requestId)return;
      if(composing||draftBusy||deleting||batchDeleting){void invoke('finish_fn_dictation',{requestId:event.requestId,transcriptId:null}).catch(()=>{});clearFnGesture(event.requestId);return;}
      fnRequest=event.requestId;fnHeld=true;await check();
      if(!alive||!fnHeld)return;
      if(status.state!=='ready'){error='Set up Amharic voice tools in Settings to use the dictation shortcut.';await finishFn();return;}
      mode='live';await start(event.requestId);
    }else if(fnRequest===event.requestId){
      fnHeld=false;
      if(event.cancelled||!recording){await cancel();await finishFn();}
      else await stop();
    }
  }
  $effect(()=>{if(fnRequest){const text=[...(session?.finalWords??[]),...(session?.partialWords??[])].map(w=>w.word).join(' ');void invoke('update_fn_overlay',{requestId:fnRequest,text:text||stage||'Listening…'}).catch(()=>{});}});
  let resampler:DictationResampler|null=null,queue:DictationSendQueue|null=null,chunks:Float32Array[]=[],frameCount=0;
  let captureRate=16000;
  let captureStopped:(()=>void)|null=null,stopping=false;
  let recordingStore:{id:string;pending:Promise<{id:string}>|null}={id:'',pending:null};
  function persistRecording(){
    const target=recordingStore;if(target.id)return Promise.resolve({id:target.id});if(target.pending)return target.pending;
    if(frameCount<captureRate/40)return Promise.reject(new Error('Recording too short'));
    const bytes=dictationWav(chunks,captureRate),name=`Dictation-${new Date().toISOString().replace(/[:.]/g,'-')}.wav`;
    target.pending=invoke<{id:string}>('import_transcription_audio',{name,bytes:Array.from(bytes)}).then(imported=>{target.id=imported.id;return imported;}).catch(error=>{target.pending=null;throw error;});return target.pending;
  }
  const delay=(ms:number)=>new Promise(resolve=>setTimeout(resolve,ms));
  const valid=(token:number)=>alive&&token===generation;
  function releaseUrl(){if(audioUrl)URL.revokeObjectURL(audioUrl);audioUrl='';}
  function closeMicrophone(){stream?.getTracks().forEach(track=>track.stop());stream=null;node?.disconnect();source?.disconnect();mute?.disconnect();if(node)node.port.onmessage=null;node=null;source=null;mute=null;if(context)void context.close().catch(()=>{});context=null;recording=false;}
  function retainAudio(){releaseUrl();if(frameCount)audioUrl=URL.createObjectURL(new Blob([dictationWav(chunks,captureRate)],{type:'audio/wav'}));}
  async function check(){try{const value=await invoke<typeof status>('get_asr_status');if(alive)status=value;}catch{if(alive)status={state:'not_installed',message:'Amharic voice tools are not available. Open setup to check them.'};}}
  async function capture(token:number){
    startupPhase='microphone';
    const next=await navigator.mediaDevices.getUserMedia(microphoneConstraints(fnRequest?'':$preferences.microphoneId));
    if(!valid(token)){next.getTracks().forEach(track=>track.stop());return;}
    startupPhase='audio';stream=next;const ctx=context??new AudioContext();context=ctx;await ctx.resume();
    if(!valid(token)){next.getTracks().forEach(track=>track.stop());void ctx.close().catch(()=>{});return;}
    startupPhase='capture';await ctx.audioWorklet.addModule('/dictation-capture.js');if(!valid(token)){next.getTracks().forEach(track=>track.stop());void ctx.close().catch(()=>{});return;}
    resampler=new DictationResampler(ctx.sampleRate,captureRate);source=ctx.createMediaStreamSource(next);node=new AudioWorkletNode(ctx,'dictation-capture',{numberOfInputs:1,numberOfOutputs:1,outputChannelCount:[1]});mute=ctx.createGain();mute.gain.value=0;
    node.port.onmessage=event=>{
      if(!valid(token))return;
      if(event.data.stopped){captureStopped?.();return;}
      if(event.data.samples){try{accept(resampler!.push(event.data.samples));}catch{void fail('Live recognition is falling behind. Your recording is kept below. Try transcription after you stop.');}}
    };
    source.connect(node);node.connect(mute);mute.connect(ctx.destination);recording=true;busy=false;stage='Listening…';
    next.getAudioTracks().forEach(track=>track.onended=()=>{if(recording&&!stopping)void stop();});
  }
  function accept(samples:Float32Array){if(!samples.length)return;chunks.push(samples);frameCount+=samples.length;seconds=frameCount/captureRate;if(mode==='live')queue?.add(samples);if(fnRequest&&starting&&frameCount>captureRate*60){void fail('Voice tools are taking too long to start. Your recording is kept below.');return;}if(frameCount>=captureRate*600&&!stopping){stage='Recording limit reached. Saving…';void stop();}}
  async function start(shortcutId=0){
    if(busy||draftBusy||deleting||batchDeleting||recording||!desktop||status.state!=='ready')return;
    const token=++generation;selectedDraft='';releaseDraft();retryingDraftId='';activeAudioId='';window.dispatchEvent(new CustomEvent('ethiopic-audio-play',{detail:'dictation-capture'}));busy=true;error='';saved=false;session=null;job=null;chunks=[];recordingStore={id:'',pending:null};frameCount=0;seconds=0;releaseUrl();stage='Preparing microphone…';
    starting=true;stopAfterStartup=false;startupPhase='audio';
    try{
      // Resume in the click gesture; request microphone only after a live model is ready.
      context=new AudioContext();if(!shortcutId)await context.resume();
      captureRate=16000;
      if(mode==='after'){try{const noise=await invoke<{enabled:boolean;ready:boolean}>('get_noise_reduction_status');if(noise?.enabled&&noise.ready)captureRate=48000;}catch{/* Ordinary recording remains available. */}}
      if(mode==='live'){
        startupPhase='voice-tools';
        const next=await invoke<Session>('start_dictation');
        if(!valid(token)){void invoke('cancel_dictation',{sessionId:next.id}).catch(()=>{});return;}
        session=next;
        captureRate=next.inputSampleRate??16000;
        if(shortcutId){await invoke('bind_fn_dictation',{requestId:shortcutId,sessionId:session.id});if(!valid(token))return;await capture(token);}
        startupPhase='voice-tools';
        while(valid(token)&&session&&!['ready','recording','failed','cancelled','completed'].includes(session.state)){stage=session.stage;await delay(400);if(!valid(token))return;session=await invoke<Session>('get_dictation_session',{sessionId:session.id});}
        if(!valid(token))return;
        if(!session||!['ready','recording'].includes(session.state))throw new Error(session?.error||'Voice tools could not get ready. Open Settings to check them, then retry.');
        const sessionId=session.id;queue=new DictationSendQueue(async(samples:number[])=>{await invoke('feed_dictation',{sessionId,samples});},captureRate*(shortcutId?60:10));
        if(shortcutId){for(const chunk of chunks)queue.add(chunk);if(stopAfterStartup){await queue.finish();session={...session,state:'finishing',stage:'Finishing transcript…'};stage=session.stage;busy=true;await invoke('stop_dictation',{sessionId});}}
      }
      if(!shortcutId)await capture(token);
    }catch(cause){if(valid(token))await fail(dictationStartupMessage(cause,startupPhase));}
    finally{if(valid(token)){starting=false;if(!recording&&!error)busy=session?.state==='finishing';}}
  }
  async function flushMicrophone(){
    if(node){const active=node;await new Promise<void>((resolve,reject)=>{const timeout=setTimeout(()=>{captureStopped=null;reject(new Error('Microphone did not finish'));},2000);captureStopped=()=>{clearTimeout(timeout);captureStopped=null;resolve();};active.port.postMessage('stop');});}
    if(resampler){accept(resampler.push([],true));resampler=null;}
    closeMicrophone();retainAudio();
  }
  async function stop(){
    if(stopping||!recording)return;stopping=true;busy=true;stage='Saving your recording…';const token=generation;
    try{
      await flushMicrophone();if(!valid(token))return;
      if(starting&&fnRequest){stopAfterStartup=true;stage='Finishing transcript…';return;}
      if(mode==='live'&&session){await queue?.finish();if(!valid(token))return;const sessionId=session.id;session={...session,state:'finishing',stage:'Finishing transcript…'};stage=session.stage;await invoke('stop_dictation',{sessionId});}
      else{
        if(frameCount<captureRate/40)throw new Error('too_short');
        const imported=await persistRecording();if(!valid(token))return;
        job=await invoke<Job>('start_transcription',{audioId:imported.id});stage=job.stage;
      }
    }catch{if(valid(token))await fail('Dictation could not finish. Your recording is kept below; save it or retry from Transcription.');}
    finally{stopping=false;}
  }
  async function fail(message:string){void finishFn();error=message;closeMicrophone();try{if(resampler){const tail=resampler.push([],true);chunks.push(tail);frameCount+=tail.length;resampler=null;}}catch{/* Capture already flushed. */}retainAudio();if(mode==='after'&&frameCount>=captureRate/40)void persistRecording().catch(()=>{});if(session&&!['completed','cancelled','failed'].includes(session.state))void invoke('cancel_dictation',{sessionId:session.id}).catch(()=>{});busy=false;stage='Recording stopped';}
  async function cancel(){
    void finishFn();const active=session?.id,recognition=job?.id;++generation;starting=false;closeMicrophone();if(!retryingDraftId)retainAudio();retryingDraftId='';resampler=null;queue=null;busy=false;stopping=false;stage='Cancelled. Your recording is kept below.';
    if(mode==='after'&&frameCount>=captureRate/40){const token=generation;void persistRecording().then(()=>{if(valid(token))stage='Cancelled. Your recording is saved in Transcription for retry.';}).catch(()=>{if(valid(token))error='Your recording could not be saved to the app. Save the WAV below before leaving this page.';});}
    if(active)void invoke('cancel_dictation',{sessionId:active}).catch(()=>{});if(recognition)void invoke('cancel_transcription',{jobId:recognition}).catch(()=>{});
    if(session)session={...session,state:'cancelled'};if(job)job={...job,state:'cancelled'};
  }
  async function poll(){
    if(polling||!alive||starting)return;polling=true;const token=generation;
    try{
      if(session&&!['completed','failed','cancelled'].includes(session.state)){
        const next=await invoke<Session>('get_dictation_session',{sessionId:session.id});if(!valid(token))return;session=next;if(!recording)stage=next.stage;
        if(next.state==='completed'){await complete(next.transcriptId);}
        else if(next.state==='failed'){await fail(next.error||'Recognition stopped. Your recording is kept below.');}
      }else if(job&&!['completed','failed','cancelled'].includes(job.state)){
        const next=await invoke<Job>('get_transcription_job',{jobId:job.id});if(!valid(token))return;job=next;stage=next.stage;
        if(next.state==='completed'){await complete(next.transcriptId);}
        else if(next.state==='failed'){busy=false;retryingDraftId='';error=next.error||'Recognition stopped. Your recording is saved in its card for retry.';}
      }
    }catch{if(valid(token)&&(session||job))error='Could not check recognition progress. Your recording is retained; try again or open Transcription.';}
    finally{polling=false;}
  }
  onMount(()=>{void check();void loadHistory();const unsubscribe=fnGesture.subscribe(value=>void shortcut(value));const timer=setInterval(()=>void poll(),500);return()=>{alive=false;generation++;unsubscribe();void finishFn();clearInterval(timer);closeMicrophone();releaseUrl();releaseDraft();if(mode==='after'&&frameCount>=captureRate/40)void persistRecording().catch(()=>{});if(session&&!['completed','failed','cancelled'].includes(session.state))void invoke('cancel_dictation',{sessionId:session.id}).catch(()=>{});if(job&&!['completed','failed','cancelled'].includes(job.state))void invoke('cancel_transcription',{jobId:job.id}).catch(()=>{});};});
  function dropRecording(event:DragEvent){if(event.target instanceof Element&&event.target.closest('textarea,input')&&!Array.from(event.dataTransfer?.types??[]).some(type=>type==='application/x-ethiopic-studio-recording'||type==='Files'))return;event.preventDefault();event.stopPropagation();try{requestRecordingTransfer('dictation',readRecordingDrop(event.dataTransfer));}catch(cause){error=cause instanceof Error?cause.message:'Drop a recording card or audio file.';}}
</script>
<div class="dictation-workspace" role="region" aria-label="Dictation workspace" ondragover={event=>event.preventDefault()} ondrop={dropRecording}>
  {#if settingsOpen}<div class="dictation-settings" id="dictation-recording-settings" role="region" aria-label="Recording options"><div class="settings-heading"><strong>Recording options</strong><button aria-label="Close recording settings" onclick={()=>settingsOpen=false}>×</button></div><label><input type="checkbox" checked={mode==='after'} disabled={busy||recording} onchange={event=>mode=event.currentTarget.checked?'after':'live'}/> Transcribe after stopping</label><p>Use this if words appear slowly while you speak. Maximum recording: 10 minutes.</p><NoiseReductionControl disabled={busy||recording}/></div>{/if}
  <RecordingWorkspace {history} {drafts} selectedId={selectedDraft||activeAudioId} bind:historyOpen onSelect={selectHistory} onDelete={requestDelete} onRefresh={loadHistory} busy={composing||draftBusy||deleting} bind:selection bind:batchBusy={batchDeleting} onBatchDelete={deleteBatch}>
    {#snippet children()}
      {#if notice}<p class="workspace-notice" role="status">{notice}</p>{/if}
      {#if historyLoading}<p class="workspace-message">Opening recordings…</p>{/if}
      {#each drafts as draft (draft.id)}<DraftRecordingCard {draft} opened={selectedDraft===draft.id} src={selectedDraft===draft.id?draftPreview:''} running={retryingDraftId===draft.id&&busy} busy={draftBusy||deleting||composing} ready={status.state==='ready'} onOpen={()=>openDraft(draft.id)} onStart={retryDraft} onDelete={()=>requestDelete(draft.id,'recording')} checked={selection.includes(recordingSelectionKey('recording',draft.id))} selectionDisabled={composing||draftBusy||deleting||batchDeleting} onChecked={checked=>toggleChecked(draft.id,'recording',checked)}/>{/each}
      {#each history as transcript,i (transcript.id)}<TranscriptCard onTranscribe={status.state==='ready'?()=>retranscribe(transcript):undefined} {transcript} number={i+1} expanded={activeAudioId===transcript.id} onToggle={()=>{if(!recording){activeAudioId=activeAudioId===transcript.id?'':transcript.id;selectedDraft='';releaseDraft();}}} onUpdated={updated} onDelete={()=>askDelete(transcript)} deleteDisabled={composing||deleting||batchDeleting} checked={selection.includes(recordingSelectionKey('transcript',transcript.id))} selectionDisabled={composing||deleting||batchDeleting} onChecked={checked=>toggleChecked(transcript.id,'transcript',checked)}/>{/each}
      {#if !history.length&&!drafts.length&&!historyLoading&&!composing}<div class="recording-empty"><h2>Your recordings, in words</h2><p>Use the microphone to create your first transcript.</p></div>{/if}
      {#if composing&&!retryingDraftId}<article class="composer recording-card" id="dictation-live" class:is-recording={recording} aria-label="Live transcript"><header><span class:recording-dot={recording}>{recording?'● Recording':stage||'Getting ready…'}</span><time>{Math.floor(seconds/60)}:{String(Math.floor(seconds%60)).padStart(2,'0')}</time></header><div class="live-text" bind:this={liveText} role="textbox" aria-label="Dictation text" aria-readonly="true" tabindex="0">{#if session?.finalWords.length||session?.partialWords.length}{#each session?.finalWords??[] as word}<span>{word.word} </span>{/each}<span class="partial">{(session?.partialWords??[]).map(word=>word.word).join(' ')}</span>{:else}<span class="placeholder">{recording?(mode==='after'?'Listening. Your transcript will appear after you stop.':'Listening…'):'Getting ready…'}</span>{/if}</div>{#if busy&&!recording}<p class="workspace-message" role="status">{stage}</p>{/if}</article>{/if}
      {#if error}<div class="workspace-error" role="alert"><p>{error}</p><button class="secondary" onclick={loadHistory}>Check again</button></div>{/if}
      {#if audioUrl&&!saved&&!recording}<article class="recovered-recording recording-card" id="dictation-recovered"><h2>Your recording</h2><WaveformPlayer src={audioUrl} label="Dictation recording" id="dictation-recording"/><a href={audioUrl} download="dictation.wav">Save recording as WAV</a></article>{/if}
    {/snippet}
    {#snippet input()}
      <RecordingInput target="dictation" onReceive={receiveRecording} disabled={composing||deleting||batchDeleting||draftBusy}/>
      {#if status.state!=='ready'}<p class="input-status" role="status">{status.state==='checking'?'Checking voice tools…':'Set up Amharic voice tools to start.'}<button onclick={onSetup}>Open setup</button></p>{/if}
      <div class="record-controls"><button class="record-button" class:stopping={recording} onclick={()=>recording?stop():start()} disabled={!desktop||(!recording&&(busy||draftBusy||deleting||batchDeleting||status.state!=='ready'))} aria-label={recording?'Stop recording':busy?'Preparing dictation':'Start recording'} title={recording?'Stop recording':'Start recording'}>{#if recording}<span class="stop-symbol" aria-hidden="true"></span>{:else}<span class="record-symbol" aria-hidden="true"></span>{/if}</button><span class="record-hint" role="status">{recording?'Tap to stop':busy?'Saving your words…':'Tap to record'}</span>{#if busy||recording}<button class="cancel-button" onclick={cancel}>Cancel</button>{/if}</div>
    {/snippet}
  </RecordingWorkspace>
</div>
<RecordingDeleteDialog target={deleteTarget} busy={deleting} error={deleteError} onCancel={closeDelete} onConfirm={deleteRecording}/>
<style>
 .composer.is-recording{border-color:var(--studio-tone-ee7586);animation:recording-border 2s ease-in-out infinite}.record-button{position:relative}.record-button.stopping{animation:recording-breathe 2s ease-in-out infinite}.record-button.stopping::before,.record-button.stopping::after{content:'';position:absolute;inset:-4px;border:1px solid var(--studio-tone-ee7586);border-radius:50%;pointer-events:none;opacity:0;animation:recording-ripple 2s ease-out infinite}.record-button.stopping::after{animation-delay:1s}
 @keyframes recording-ripple{0%{transform:scale(1);opacity:.6}100%{transform:scale(1.55);opacity:0}}@keyframes recording-breathe{0%,100%{box-shadow:0 0 0 5px var(--studio-tone-d84a5e12)}50%{box-shadow:0 0 0 8px var(--studio-tone-d84a5e12)}}@keyframes recording-border{0%,100%{box-shadow:0 0 0 0 var(--studio-tone-d84a5e12)}50%{box-shadow:0 0 0 3px var(--studio-tone-d84a5e12)}}
 @media(prefers-reduced-motion:reduce){.composer.is-recording,.record-button.stopping{animation:none;transform:none}.record-button.stopping::before,.record-button.stopping::after{display:none;animation:none}}

.dictation-workspace{position:relative;height:100%;min-height:0}.composer,.recovered-recording{border:1px solid var(--studio-tone-26354b);border-radius:14px;background:var(--studio-tone-0d1728);padding:20px 22px;flex-shrink:0}.composer header{display:flex;align-items:center;justify-content:space-between;font-size:12px;color:var(--studio-tone-97a8c3)}.composer time{font-variant-numeric:tabular-nums}.recording-dot{color:var(--studio-tone-ee9db0)}.live-text{min-height:110px;max-height:360px;overflow:auto;margin-top:16px;font-size:21px;line-height:1.85;white-space:pre-wrap;overflow-wrap:anywhere}.partial,.placeholder{color:var(--studio-tone-7f8da6)}.placeholder{font-size:16px}.record-controls{display:flex;flex-direction:column;align-items:center;justify-content:center;gap:10px;position:relative;min-height:84px;width:100%}.record-button{display:grid;place-items:center;width:54px;height:54px;padding:0;border:1px solid var(--studio-tone-ee7586);background:var(--studio-tone-d84a5e);box-shadow:0 0 0 5px var(--studio-tone-d84a5e12);border-radius:50%;cursor:pointer}.record-button:hover{background:var(--studio-tone-ec6072)}.record-symbol{width:17px;height:17px;border-radius:50%;background:var(--studio-tone-fff3f4)}.stop-symbol{width:17px;height:17px;border-radius:3px;background:var(--studio-tone-fff3f4)}.record-button:disabled{opacity:.45;cursor:default}.record-hint{text-align:center;font-size:12px;color:var(--studio-tone-98a8c2)}.cancel-button{position:absolute;right:0;top:18px;background:transparent;border:0;padding:8px;color:var(--studio-tone-a5b3c9);font:inherit;font-size:12px;cursor:pointer}.dictation-settings{position:absolute;right:26px;top:10px;z-index:10;width:min(310px,calc(100% - 52px));box-sizing:border-box;padding:16px;background:var(--studio-tone-111b2d);border:1px solid var(--studio-tone-35455c);border-radius:12px;box-shadow:0 14px 36px var(--studio-tone-0006);font-size:11px;color:var(--studio-tone-9eafc9)}.settings-heading{display:flex;align-items:center;justify-content:space-between;font-size:13px;color:var(--studio-tone-e9eefb)}.settings-heading button{border:0;background:transparent;color:var(--studio-tone-a8b6cb);font-size:22px;padding:0 3px;cursor:pointer}.dictation-settings label{display:flex;align-items:center;gap:8px;padding-top:14px;color:var(--studio-tone-a8b6cb);font-size:12px}.dictation-settings input{accent-color:var(--studio-tone-d84a5e)}.dictation-settings p{font-size:11px;line-height:1.7;margin:8px 0}.input-status{font-size:12px;color:var(--studio-tone-a8b8d1);text-align:center;margin:0 0 12px}.input-status button{font:inherit;font-size:11px;padding:3px 5px;background:transparent;color:var(--studio-tone-7bddb8);text-decoration:underline}.recovered-recording h2{font-size:13px;font-weight:500;color:var(--studio-tone-becce0);margin-top:0}.recovered-recording a{display:inline-block;color:var(--studio-tone-71d8b0);font-size:12px;margin-top:12px}button:focus-visible,a:focus-visible,input:focus-visible,.live-text:focus-visible{outline:2px solid var(--studio-gold);outline-offset:3px}@media(max-width:650px){.composer,.recovered-recording{padding:16px}.live-text{font-size:20px}}
</style>
