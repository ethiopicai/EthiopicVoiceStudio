<script lang="ts">
  import NoiseReductionControl from './NoiseReductionControl.svelte';
  import {prepareRecordingFile} from './recording-transfer.js';
  import { onMount, tick } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import {recordingTitles,displayRecordingTitle,forgetRecordingTitles} from './recording-titles.js';
  import {recordingSelectionKey,toggleRecordingSelection} from './recording-selection.js';
  import type {RecordingDeleteItem,RecordingBatchDeleteResult} from './recording-selection-types';
  import { transcriptionFileError, isActiveJob } from './transcript.js';
  import type { Transcript } from './transcript-types';
  import TranscriptCard from './TranscriptCard.svelte';
  import RecordingWorkspace from './RecordingWorkspace.svelte';
  import {scrollToWorkspaceCard} from './card-navigation.js';
  import RecordingDeleteDialog from './RecordingDeleteDialog.svelte';
  import DraftRecordingCard from './DraftRecordingCard.svelte';
  import RecordingInput from './RecordingInput.svelte';
  import {readRecordingDrop,requestRecordingTransfer} from './recording-transfer.js';
  import {importRecordingHistory} from './recording-transfer.js';
  let { onSetup, historyOpen = $bindable(true) }: { onSetup:()=>void; historyOpen?:boolean } = $props();
  let selection=$state<string[]>([]),batchDeleting=$state(false);
  function toggleChecked(id:string,kind:'transcript'|'recording',checked:boolean){selection=toggleRecordingSelection(selection,kind,id,checked);}
  type Job={id:string;state:string;stage:string;error?:string;transcriptId?:string};
  let status=$state<{state:string;message:string;modelVersion?:string}>({state:'checking',message:'Checking local voice tools…'});
  let history=$state<Transcript[]>([]);
  let recordings=$state<{id:string;name:string}[]>([]);
  const drafts=$derived(recordings.filter(item=>!history.some(transcript=>transcript.source_recording_id===item.id)));
  let expandedId=$state<string|null>(null);
  let imported=$state<{id:string;name:string}|null>(null);
  let pendingOpen=$state(true);
  let previewUrl=$state('');
  let job=$state<Job|null>(null);
  let error=$state('');
  let busy=$state(false);
  let fileInput=$state<HTMLInputElement>();
  type DeleteTarget={kind:'transcript'|'recording';id:string;name:string;revision?:number};
  let deleteTarget=$state<DeleteTarget|null>(null);
  let deleting=$state(false),notice=$state(''),deleteError=$state('');
  $effect(()=>{
    if(!notice)return;
    const timer=setTimeout(()=>notice='',4000);
    return()=>clearTimeout(timer);
  });
  let historyRequest=0,recordingsRequest=0;
  function askDelete(target:DeleteTarget){
    if(busy||active||deleting||batchDeleting)return;
    deleteError='';deleteTarget=target;
  }
  function closeDelete(){if(!deleting){deleteTarget=null;deleteError='';}}
  async function confirmDelete(){
    if(!deleteTarget||deleting||busy||active)return;
    const target=deleteTarget;deleting=true;error='';notice='';opening++;historyRequest++;recordingsRequest++;
    try{
      const removed=target.kind==='transcript'
        ?await invoke<string[]>('delete_transcript',{id:target.id,revision:target.revision})
        :(await invoke('delete_transcription_recording',{audioId:target.id}),[target.id]);
      forgetRecordingTitles([{kind:target.kind,id:target.id},...removed.map(id=>({kind:'recording',id}))]);
      if(!alive)return;
      if(target.kind==='transcript'){
        history=history.filter(item=>item.id!==target.id);
        if(expandedId===target.id)expandedId=null;
        if(job?.transcriptId===target.id)job=null;
      }
      recordings=recordings.filter(item=>!removed.includes(item.id));
      if(imported&&removed.includes(imported.id)){releasePreview();imported=null;job=null;}
      notice='Transcription deleted from this app. Your original file is unchanged.';
      deleting=false;closeDelete();
    }catch{
      if(alive){
        deleteError='Deletion could not finish. Close active voice tasks and try again. Your original files are unchanged.';
        deleting=false;
        await Promise.allSettled([loadHistory(),loadRecordings()]);
      }
    }finally{deleting=false;}
  }
  async function deleteBatch(items:RecordingDeleteItem[]):Promise<RecordingBatchDeleteResult>{
    const result=await invoke<RecordingBatchDeleteResult>('delete_transcripts',{items});
    forgetRecordingTitles([...result.deleted,...result.removedRecordingIds.map(id=>({kind:'recording',id}))]);
    if(!alive)return result;opening++;historyRequest++;recordingsRequest++;
    const deleted=new Set(result.deleted.filter(item=>item.kind==='transcript').map(item=>item.id));
    const removed=new Set([...result.removedRecordingIds,...result.deleted.filter(item=>item.kind==='recording').map(item=>item.id)]);
    history=history.filter(item=>!deleted.has(item.id));recordings=recordings.filter(item=>!removed.has(item.id));
    if(expandedId&&deleted.has(expandedId))expandedId=null;
    if(job?.transcriptId&&deleted.has(job.transcriptId))job=null;
    if(imported&&removed.has(imported.id)){releasePreview();imported=null;job=null;}
    return result;
  }
  let alive=true;
  let opening=0;
  const active=$derived(job!==null && isActiveJob(job.state));
  const desktop='__TAURI_INTERNALS__' in window;
  function releasePreview(){if(previewUrl)URL.revokeObjectURL(previewUrl);previewUrl='';}
  function updated(value:Transcript){history=history.map(item=>item.id===value.id?value:item);}
  async function loadHistory(){const request=++historyRequest;const next=await invoke<Transcript[]>('list_transcripts');if(alive&&request===historyRequest)history=next.sort((a,b)=>(a.created_at_ms??0)-(b.created_at_ms??0));}
  async function loadRecordings(){const request=++recordingsRequest;const next=await invoke<typeof recordings>('list_transcription_recordings');if(alive&&request===recordingsRequest)recordings=next;}
  async function refreshStatus(){
    status={state:'checking',message:'Checking local voice tools…'};
    try{const next=await invoke<typeof status>('get_asr_status');if(alive)status=next;}
    catch{if(alive)status={state:'not_installed',message:'Voice tools are unavailable. Check setup, then check again.'};}
  }
  async function openTranscript(id:string){
    imported=null;pendingOpen=false;releasePreview();
    if(expandedId===id){await tick();scrollToWorkspaceCard(`transcript-${id}`);return;}
    const request=++opening;
    try{
      const next=await invoke<Transcript>('get_transcript',{id});
      if(!alive || request!==opening)return;
      updated(next);expandedId=id;
      await tick();
      scrollToWorkspaceCard(`transcript-${id}`);
    }catch{if(alive)error='This transcript could not be reopened. Your saved history is unchanged.';}
  }
  function toggleCard(id:string){
    if(expandedId===id){opening++;expandedId=null;}
    else void openTranscript(id);
  }
  async function openRecording(id:string){
    if(busy||active||deleting||batchDeleting)return;
    opening++;busy=true;error='';
    try{
      const source=await invoke<{name:string;bytes:number[]}>('get_transcription_recording_audio',{audioId:id});
      if(!alive)return;
      releasePreview();previewUrl=URL.createObjectURL(new Blob([new Uint8Array(source.bytes)],{type:'audio/wav'}));
      imported={id,name:source.name};pendingOpen=true;job=null;expandedId=null;
      await tick();scrollToWorkspaceCard(`draft-${id}`);
    }catch{error='This recording could not be opened. Please retry.';}
    finally{busy=false;}
  }
  async function receiveRecording(value:import('./recording-transfer.js').RecordingPackage,input?:import('./recording-transfer.js').RecordingInput){if(busy||active||deleting||batchDeleting)throw Error('Finish the current task before importing another recording.');busy=true;error='';try{const received=await importRecordingHistory(value,input);if(!alive)return;await Promise.all([loadHistory(),loadRecordings()]);if(received.transcript){expandedId=received.transcript.id;imported=null;releasePreview();}else if(received.imported){imported=received.imported;expandedId=null;pendingOpen=true;releasePreview();previewUrl=URL.createObjectURL(new Blob([new Uint8Array(value.bytes)],{type:'audio/wav'}));}}finally{if(alive)busy=false;}}
  async function choose(file?:File){
    if(!file||busy||active||deleting||batchDeleting||!desktop)return;
    error=transcriptionFileError(file);if(error)return;
    opening++;busy=true;
    try{
      file=await prepareRecordingFile(file);
      const next=await invoke<{id:string;name:string}>('import_transcription_audio',{name:file.name,bytes:Array.from(new Uint8Array(await file.arrayBuffer()))});
      if(!alive)return;
      releasePreview();recordings=[next,...recordings.filter(item=>item.id!==next.id)];previewUrl=URL.createObjectURL(file);imported=next;pendingOpen=true;job=null;expandedId=null;
      try{await loadRecordings();}catch{error='Your transcription card is saved; the history could not refresh.';}
    }catch(cause){error=typeof cause==='string'?cause:cause instanceof Error?cause.message:'We could not import this recording. Check the file and available storage.';}
    finally{busy=false;if(fileInput)fileInput.value='';}
  }
  async function refreshHistory(){
    try{await Promise.all([loadHistory(),loadRecordings()]);error='';}catch{error='History could not refresh. Your saved transcripts are unchanged.';}
  }
  async function finished(next:Job){
    if(next.state==='completed'&&next.transcriptId){
      const request=++opening;
      try{await loadHistory();}
      catch{
        try{
          const saved=await invoke<Transcript>('get_transcript',{id:next.transcriptId});
          if(!alive || request!==opening)return;
          history=[saved,...history.filter(item=>item.id!==saved.id)];
          error='Your transcript was saved. Refresh transcripts to reload the complete history.';
        }catch{error='Recognition finished, but the saved transcript could not be opened. Use Refresh transcripts to retry.';return;}
      }
      if(!alive || request!==opening)return;
      const sourceId=imported?.id;if(sourceId){recordingsRequest++;recordings=recordings.filter(item=>item.id!==sourceId);}
      releasePreview();imported=null;expandedId=next.transcriptId;
      void loadRecordings().catch(()=>{});
      await tick();scrollToWorkspaceCard(`transcript-${next.transcriptId}`);
    }else if(next.state==='failed')error=next.error||'Recognition failed. Your recording is saved for retry.';
  }
  async function start(){
    if(!imported||busy||active||deleting||batchDeleting||status.state!=='ready')return;
    busy=true;error='';
    try{job=await invoke<Job>('start_transcription',{audioId:imported.id});await finished(job);}
    catch{error='Recognition could not start. Check voice tools, then try again.';}
    finally{busy=false;}
  }
  async function retranscribe(transcript:Transcript){if(!transcript.source_recording_id||busy||active||deleting||batchDeleting)return;imported={id:transcript.source_recording_id,name:transcript.name};await start();}
  async function cancel(){if(job)try{await invoke('cancel_transcription',{jobId:job.id});}catch{error='Could not cancel yet. Please try again.';}}
  async function poll(){
    if(!job||!active||!alive)return;
    try{const next=await invoke<Job>('get_transcription_job',{jobId:job.id});if(!alive)return;job=next;await finished(next);}
    catch{error='Progress could not refresh. Your recording is still saved.';}
  }
  onMount(()=>{
    alive=true;
    if(!desktop){status={state:'not_installed',message:'Open the desktop app to use local transcription.'};return;}
    void refreshStatus();
    void loadHistory().then(()=>{if(alive&&!expandedId)expandedId=history.at(-1)?.id??null;}).catch(()=>error='History could not be loaded. Reopen this workspace to retry.');
    void loadRecordings().catch(()=>error='Saved recordings could not be listed. Reopen this workspace to retry.');
    invoke<Job|null>('get_active_transcription_job').then(value=>{if(alive&&value)job=value;}).catch(()=>{});
    let polling=false;
    const timer=setInterval(async()=>{if(polling)return;polling=true;try{await poll();}finally{polling=false;}},750);
    return()=>{alive=false;opening++;clearInterval(timer);releasePreview();};
  });
  function selectHistory(id:string,kind:'transcript'|'recording'){if(kind==='recording')void openRecording(id);else void openTranscript(id);}
  function requestDelete(id:string,kind:'transcript'|'recording'){
    const target=kind==='recording'?drafts.find(item=>item.id===id):history.find(item=>item.id===id);
    if(target)askDelete({kind,id,name:displayRecordingTitle($recordingTitles,kind,id,target.name),...(kind==='transcript'?{revision:(target as Transcript).revision}:{})});
  }
  function dropRecording(event:DragEvent){if(event.target instanceof Element&&event.target.closest('textarea,input')&&!Array.from(event.dataTransfer?.types??[]).some(type=>type==='application/x-ethiopic-studio-recording'||type==='Files'))return;event.preventDefault();event.stopPropagation();try{requestRecordingTransfer('transcription',readRecordingDrop(event.dataTransfer));}catch(cause){error=cause instanceof Error?cause.message:'Drop a recording card or audio file.';}}
</script>
<div class="transcription-root" role="region" aria-label="Transcription workspace" ondragover={event=>event.preventDefault()} ondrop={dropRecording}>
  <RecordingWorkspace {history} {drafts} selectedId={imported?.id??expandedId??''} bind:historyOpen onSelect={selectHistory} onDelete={requestDelete} onRefresh={refreshHistory} busy={busy||active||deleting} bind:selection bind:batchBusy={batchDeleting} onBatchDelete={deleteBatch}>
    {#snippet children()}
      {#if notice}<p class="workspace-notice" role="status">{notice}</p>{/if}
      {#if error}<p class="workspace-error" role="alert">{error}</p>{/if}
      {#if job&&active}<div class="workspace-progress" role="status"><span>{job.stage||job.state}</span><progress aria-label="Transcription in progress"></progress><button class="secondary" onclick={cancel}>Cancel</button></div>{/if}
      {#each drafts as item (item.id)}<DraftRecordingCard draft={item} opened={imported?.id===item.id&&pendingOpen} src={imported?.id===item.id?previewUrl:''} running={imported?.id===item.id&&active} busy={busy||active||deleting} ready={status.state==='ready'} onOpen={()=>{if(imported?.id===item.id)pendingOpen=!pendingOpen;else void openRecording(item.id);}} onStart={start} onDelete={()=>requestDelete(item.id,'recording')} checked={selection.includes(recordingSelectionKey('recording',item.id))} selectionDisabled={busy||active||deleting||batchDeleting} onChecked={checked=>toggleChecked(item.id,'recording',checked)}/>{/each}
      {#each history as item,i (item.id)}<TranscriptCard onTranscribe={status.state==='ready'?()=>retranscribe(item):undefined} transcript={item} number={i+1} expanded={expandedId===item.id} onToggle={()=>toggleCard(item.id)} onUpdated={updated} onDelete={()=>requestDelete(item.id,'transcript')} deleteDisabled={busy||active||deleting||batchDeleting} checked={selection.includes(recordingSelectionKey('transcript',item.id))} selectionDisabled={busy||active||deleting||batchDeleting} onChecked={checked=>toggleChecked(item.id,'transcript',checked)}/>{/each}
      {#if !history.length&&!drafts.length}<div class="recording-empty"><h2>Your recordings, in words</h2><p>Import audio to create your first transcript.</p></div>{/if}
    {/snippet}
    {#snippet input()}
      <RecordingInput target="transcription" onReceive={receiveRecording} disabled={busy||active||deleting||batchDeleting}/>
      <NoiseReductionControl disabled={busy||active||deleting||batchDeleting}/>
      {#if status.state!=='ready'}<p class="input-status" role="status">{status.state==='checking'?'Checking voice tools…':'Set up Amharic voice tools to start.'}<button onclick={onSetup}>Open setup</button><button onclick={refreshStatus} disabled={status.state==='checking'}>Check again</button></p>{/if}
      <input class="audio-file-input" tabindex="-1" type="file" accept=".wav,.mp3,.flac,.ogg,.m4a" bind:this={fileInput} disabled={!desktop||busy||active||deleting||batchDeleting} aria-label="Choose audio recording" onchange={()=>choose(fileInput?.files?.[0])}/>
      <div class="source-controls"><button class="import-button" aria-label="Import recording" title="Import recording" onclick={()=>fileInput?.click()} disabled={!desktop||busy||active||deleting||batchDeleting}><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 16V3m-4 4 4-4 4 4M4 15v6h16v-6"/></svg></button><span>{busy?'Opening recording…':'Choose or drop an audio recording'}</span></div>
    {/snippet}
  </RecordingWorkspace>
</div>
<RecordingDeleteDialog target={deleteTarget} busy={deleting} error={deleteError} onCancel={closeDelete} onConfirm={confirmDelete}/>
<style>
.transcription-root{height:100%;min-height:0}.audio-file-input{position:absolute;width:1px;height:1px;opacity:0;overflow:hidden;clip-path:inset(50%)}.source-controls{display:flex;flex-direction:column;align-items:center;justify-content:center;gap:10px;min-height:84px;width:100%}.source-controls>span{font-size:12px;color:var(--studio-tone-98a8c2)}.import-button{width:54px;height:54px;display:grid;place-items:center;padding:0;border:1px solid var(--studio-tone-54dcb0);background:var(--studio-tone-208c71);border-radius:50%;box-shadow:0 0 0 5px var(--studio-tone-37d39c12)}.import-button:hover{background:var(--studio-tone-2da986)}.import-button svg{width:23px;height:23px;fill:none;stroke:var(--studio-tone-e6fff5);stroke-width:1.8;stroke-linecap:round;stroke-linejoin:round}.input-status{font-size:12px;color:var(--studio-tone-a8b8d1);text-align:center;margin:0 0 12px}.input-status button{font:inherit;font-size:11px;padding:3px 5px;background:transparent;color:var(--studio-tone-7bddb8);text-decoration:underline}
</style>
