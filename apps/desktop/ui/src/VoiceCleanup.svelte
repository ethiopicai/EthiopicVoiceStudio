<script lang="ts">
  import {prepareRecordingFile} from './recording-transfer.js';
  import { onMount, tick } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import WaveformPlayer from './WaveformPlayer.svelte';
  import HistoryPane from './HistoryPane.svelte';
  import HistoryEntry from './HistoryEntry.svelte';
  import {recordingTitles,displayRecordingTitle,forgetRecordingTitles,refreshRecordingTitles} from './recording-titles.js';
  import WorkspaceCard from './WorkspaceCard.svelte';
  import {scrollToWorkspaceCard,workspaceCardId} from './card-navigation.js';
  import RecordingPanel from './RecordingPanel.svelte';
  import HistorySelection from './HistorySelection.svelte';
  import {recordingSelectionKey,toggleRecordingSelection} from './recording-selection.js';
  import type {HistoryDeleteItem,HistoryDeleteResult} from './history-selection-types';
  import RecordingInput from './RecordingInput.svelte';
  import {readRecordingDrop,requestRecordingTransfer} from './recording-transfer.js';
  import RecordingActions from './RecordingActions.svelte';
  import {recordingFile,recordingRegions} from './recording-transfer.js';
  import { transcriptionFileError, isActiveJob } from './transcript.js';
  let { recording=$bindable<File|null>(null),jobId=$bindable('') }: {recording:File|null;jobId?:string}=$props();
  type Source={id:string;name:string};
  type Result=Source & {model_version?:string;sample_rate?:number;channels?:number;frames?:number};
  type Job={id:string;audioId?:string;name?:string;state:string;stage:string;progress?:number|null;error?:string|null;resultId?:string|null};
  let status=$state<{state:string;message:string;modelVersion?:string|null}>({state:'checking',message:'Checking local voice tools…'});
  let input=$state<HTMLInputElement>();
  let imported=$state<Source|null>(null), result=$state<Result|null>(null);
  let recordings=$state<Source[]>([]),results=$state<Result[]>([]);
  let cardCollapsed=$state(false);
  let historyLoading=$state(false),historyError=$state('');
  let historySelected=$state<string[]>([]),deleteBusy=$state(false);
  const historyEntries=$derived([
    ...(result&&!results.some(item=>item.id===result?.id)?[result,...results]:results).map(item=>({...item,name:displayRecordingTitle($recordingTitles,'cleanup-result',item.id,item.name),kind:'result' as const})),
    ...(imported&&!recordings.some(item=>item.id===imported?.id)?[imported,...recordings]:recordings).map(item=>({...item,name:displayRecordingTitle($recordingTitles,'cleanup-recording',item.id,item.name),kind:'recording' as const}))
  ].sort((a,b)=>Number(active&&b.kind==='recording'&&b.id===job?.audioId)-Number(active&&a.kind==='recording'&&a.id===job?.audioId)||b.id.localeCompare(a.id,undefined,{numeric:true})));
  const selectionEntries=$derived(historyEntries.map(entry=>({id:entry.id,name:entry.name,kind:entry.kind==='result'?'cleanup-result':'cleanup-recording'})));
  let originalUrl=$state(''),cleanedUrl=$state('');
  let sourceMedia=$state<import('./recording-transfer.js').RecordingContext|null>(null);
  let error=$state(''),message=$state(''),busy=$state(false),exporting=$state(false);
  let job=$state<Job|null>(null);
  const active=$derived(job!==null&&isActiveJob(job.state));
  const desktop='__TAURI_INTERNALS__' in window;
  let restoring=$state(desktop);
  let restoreDone:()=>void=()=>{};
  const restoreReady=new Promise<void>(resolve=>restoreDone=resolve);
  let alive=true,selection=0,jobSelection=0,handledJob='';
  function releaseAudio(){if(originalUrl)URL.revokeObjectURL(originalUrl);if(cleanedUrl)URL.revokeObjectURL(cleanedUrl);originalUrl='';cleanedUrl='';}
  function blobUrl(bytes:number[]){return URL.createObjectURL(new Blob([new Uint8Array(bytes)],{type:'audio/wav'}));}
  async function refreshStatus(){
    status={state:'checking',message:'Checking local voice tools…'};
    try{const next=await invoke<typeof status>('get_cleanup_status');if(alive)status=next;}
    catch{if(alive)status={state:'not_installed',message:'Voice Cleanup needs its voice tools installed. Check again after setup.'};}
  }
  async function loadResults(){const next=await invoke<Result[]>('list_cleanup_results');if(alive)results=next;}
  async function loadRecordings(){const next=await invoke<Source[]>('list_cleanup_recordings');if(alive)recordings=next;}
  async function refreshHistory(){
    void refreshRecordingTitles();
    if(historyLoading||!desktop)return;historyLoading=true;historyError='';
    const updates=await Promise.allSettled([loadResults(),loadRecordings()]);
    if(alive){historyLoading=false;if(updates.some(update=>update.status==='rejected'))historyError='Some history could not be loaded. Your saved recordings are unchanged. Try Refresh again.';}
  }
  async function deleteHistory(items:HistoryDeleteItem[]):Promise<HistoryDeleteResult>{
    const deleted=await invoke<HistoryDeleteResult>('delete_voice_history',{items});
    forgetRecordingTitles(deleted.deleted);
    if(!alive)return deleted;
    const removed=(kind:string,id:string|undefined)=>!!id&&deleted.deleted.some(item=>item.kind===kind&&item.id===id);
    if(removed('cleanup-result',result?.id)||removed('cleanup-recording',imported?.id)){selection++;releaseAudio();result=null;imported=null;recording=null;sourceMedia=null;error='';message='';}
    if(removed('cleanup-recording',job?.audioId)||removed('cleanup-result',job?.resultId??undefined)){job=null;jobId='';}
    recordings=recordings.filter(item=>!removed('cleanup-recording',item.id));results=results.filter(item=>!removed('cleanup-result',item.id));
    return deleted;
  }
  async function selectHistory(entry:{id:string;kind:'recording'|'result'}){if(restoring||busy||active||deleteBusy)return;cardCollapsed=false;if(!currentEntry(entry)||!originalUrl){if(entry.kind==='result')await openResult(entry.id);else await openRecording(entry.id);}await tick();if(alive)scrollToWorkspaceCard(workspaceCardId(`cleanup-${entry.kind}`,entry.id));}
  function currentEntry(entry:{id:string;kind:'recording'|'result'}){return entry.kind==='result'?result?.id===entry.id:!result&&imported?.id===entry.id;}
  function toggleCard(entry:{id:string;kind:'recording'|'result'}){if(restoring||busy||active||deleteBusy)return;if(currentEntry(entry)&&!cardCollapsed){cardCollapsed=true;releaseAudio();}else void selectHistory(entry);}
  async function openResult(id:string,metadata?:Result,fromJob=false){
    const token=++selection;busy=true;error='';message='';
    try{
      const [original,cleaned]=await Promise.all([
        invoke<{name:string;bytes:number[];sourceMedia?:import('./recording-transfer.js').RecordingContext}>('get_cleanup_audio',{id,kind:'original'}),
        invoke<{name:string;bytes:number[];sourceMedia?:import('./recording-transfer.js').RecordingContext}>('get_cleanup_audio',{id,kind:'cleaned'})
      ]);
      if(!alive||token!==selection)return;
      releaseAudio();cardCollapsed=false;sourceMedia=original.sourceMedia??null;originalUrl=blobUrl(original.bytes);cleanedUrl=blobUrl(cleaned.bytes);
      result=metadata??results.find(item=>item.id===id)??{id,name:original.name};imported=null;recording=null;
      if(!fromJob){job=null;jobId='';}
    }catch{if(alive&&token===selection)error='This saved result could not be opened. Your original and cleaned recordings remain saved; select the result to retry.';}
    finally{if(alive&&token===selection)busy=false;}
  }
  async function openRecording(id:string,fromJob=false){
    if(!fromJob&&(busy||active||restoring||deleteBusy))return;
    const token=++selection;busy=true;error='';message='';
    if(fromJob)jobSelection=token;
    try{
      const source=await invoke<{name:string;bytes:number[];sourceMedia?:import('./recording-transfer.js').RecordingContext}>('get_cleanup_recording_audio',{audioId:id});
      if(!alive||token!==selection)return;
      releaseAudio();cardCollapsed=false;sourceMedia=source.sourceMedia??null;originalUrl=blobUrl(source.bytes);imported={id,name:source.name};result=null;recording=null;if(!fromJob){job=null;jobId='';}
    }catch{if(alive&&token===selection)error='This recording could not be opened. Select it again to retry.';}
    finally{if(alive&&token===selection)busy=false;}
  }
  async function receiveRecording(value:import('./recording-transfer.js').RecordingPackage){await restoreReady;if(!alive)throw Error('Reopen Voice Cleanup and retry this import.');await choose(recordingFile(value),value.context);}
  async function choose(file?:File,transferredMedia?:import('./recording-transfer.js').RecordingContext){
    if(!file||busy||active||restoring||deleteBusy||!desktop){if(transferredMedia)throw Error('Finish the current cleanup task before importing another recording.');return;}
    error=transcriptionFileError(file);if(error){if(transferredMedia)throw Error(error);return;}
    const token=++selection;busy=true;message='';
    try{
      file=await prepareRecordingFile(file);
      const next=await invoke<Source>('import_cleanup_audio',{name:file.name,bytes:Array.from(new Uint8Array(await file.arrayBuffer())),...(transferredMedia?{sourceMedia:transferredMedia}:{})});
      if(!alive||token!==selection)return;
      releaseAudio();cardCollapsed=false;sourceMedia=transferredMedia??null;originalUrl=URL.createObjectURL(file);imported=next;result=null;recording=file;job=null;jobId='';
      try{await loadRecordings();}catch{if(alive)message='Recording imported and saved. Reopen this workspace to refresh the recording list.';}
    }catch(cause){if(alive&&token===selection)error=typeof cause==='string'?cause:cause instanceof Error?cause.message:'Could not import this recording. Use mono or stereo audio at 8–48 kHz, at least 0.1 seconds long, and check free storage.';if(transferredMedia)throw Error(error);}
    finally{if(alive&&token===selection){busy=false;if(input)input.value='';}}
  }
  async function finished(next:Job){
    if(next.state==='completed'&&next.resultId&&handledJob!==next.id){
      handledJob=next.id;
      // Open success before optional history refresh, while respecting a newer
      // explicit result selection made during this job.
      if(selection===jobSelection)await openResult(next.resultId,{id:next.resultId,name:next.name??imported?.name??'Cleaned recording',model_version:status.modelVersion??undefined},true);
      try{await loadResults();}catch{if(alive)message='Cleanup finished and the result is saved. Reopen this workspace to refresh saved results.';}
    }else if(next.state==='failed')error=next.error||'Cleanup could not finish. Your original recording is saved; you can try again.';
  }
  async function start(){
    if(!imported||busy||active||restoring||deleteBusy||status.state!=='ready')return;
    busy=true;error='';message='';jobSelection=selection;const token=selection;
    try{const next=await invoke<Job>('start_cleanup',{audioId:imported.id});if(!alive)return;job=next;jobId=next.id;await finished(next);}
    catch{if(alive)error='Cleanup could not start. Check voice tools and try again. Your original is safe.';}
    finally{if(alive&&selection===token)busy=false;}
  }
  async function poll(){
    if(!job||!active||!alive)return;
    const id=job.id;
    try{const next=await invoke<Job>('get_cleanup_job',{jobId:id});if(!alive||job?.id!==id)return;job=next;await finished(next);}
    catch{if(alive)error='Progress could not refresh. Your original recording is safe; processing may still be continuing.';}
  }
  async function cancel(){if(!job)return;try{await invoke('cancel_cleanup',{jobId:job.id});}catch{if(alive)error='Could not cancel yet. Please try again.';}}
  async function exportResult(){
    if(!result||exporting)return;exporting=true;error='';message='';
    try{const output=await invoke<{name:string;message:string}>('export_cleanup_audio',{id:result.id});if(alive)message=output.message;}
    catch{if(alive)error='The cleaned recording could not be exported. Check your Downloads folder permissions and free space, then retry.';}
    finally{if(alive)exporting=false;}
  }
  onMount(()=>{
    alive=true;
    if(!desktop){status={state:'not_installed',message:'Open the desktop app to clean up recordings locally.'};restoreDone();return;}
    void refreshStatus();
    void refreshHistory();
    void (async()=>{
      try{
        let next:Job|null=null;
        if(jobId){try{next=await invoke<Job>('get_cleanup_job',{jobId});}catch{/* A restarted app may no longer have the remembered job. */}}
        if(!next)next=await invoke<Job|null>('get_active_cleanup_job');
        if(!alive||!next)return;
        job=next;jobId=next.id;jobSelection=selection;
        if(next.state==='completed'){await finished(next);return;}
        if(next.audioId){imported={id:next.audioId,name:next.name??'Your recording'};result=null;await openRecording(next.audioId,true);jobSelection=selection;}
        if(alive&&job?.id===next.id)await finished(job);
      }catch{if(alive)error='The cleanup task could not be restored. Reopen Voice Cleanup to try again; processing may still be continuing.';}
      finally{if(alive)restoring=false;restoreDone();}
    })();
    let polling=false;
    const timer=setInterval(async()=>{if(polling||restoring)return;polling=true;try{await poll();}finally{polling=false;}},750);
    return()=>{alive=false;selection++;clearInterval(timer);releaseAudio();restoreDone();};
  });
  function dropRecording(event:DragEvent){if(event.target instanceof Element&&event.target.closest('textarea,input')&&!Array.from(event.dataTransfer?.types??[]).some(type=>type==='application/x-ethiopic-studio-recording'||type==='Files'))return;event.preventDefault();event.stopPropagation();if(busy||active||restoring||deleteBusy)return;try{requestRecordingTransfer('cleanup',readRecordingDrop(event.dataTransfer));}catch(cause){error=cause instanceof Error?cause.message:'Drop a recording card or audio file.';}}
</script>

<section class="cleanup-workspace" aria-label="Voice Cleanup workspace" ondragover={event=>{event.preventDefault();if((busy||active||restoring)&&event.dataTransfer)event.dataTransfer.dropEffect='none';}} ondrop={dropRecording}>
  <HistoryPane id="cleanup-history" label="Cleanup history" storageKey="ethiopic.cleanup">
    {#snippet controls()}
      <div class="cleanup-history-tools"><span>{historyEntries.length} saved</span><button class="secondary" disabled={historyLoading||restoring||busy||active||deleteBusy} onclick={refreshHistory} aria-label="Refresh cleanup history">{historyLoading?'Loading…':'Refresh'}</button></div>
      <HistorySelection entries={selectionEntries} bind:selected={historySelected} bind:busy={deleteBusy} disabled={restoring||busy||active} onDelete={deleteHistory}/>
      {#if historyError}<p class="cleanup-history-error" role="alert">{historyError}</p>{/if}
    {/snippet}
    {#snippet history()}
      {#if historyLoading&&!historyEntries.length}<p class="cleanup-history-empty" role="status">Loading recordings…</p>{:else if !historyEntries.length}<p class="cleanup-history-empty">No cleanup recordings yet. Import a recording to get started.</p>{/if}
      <div class="cleanup-history-list">{#each historyEntries as entry (`${entry.kind}-${entry.id}`)}
        {@const selected=entry.kind==='result'?result?.id===entry.id:imported?.id===entry.id}
        <HistoryEntry class="cleanup-history-item" name={entry.name} detail={entry.kind==='result'?'Cleaned recording':active&&entry.id===job?.audioId?'Cleaning…':'Original recording'} {selected} disabled={restoring||busy||active||deleteBusy} checked={historySelected.includes(recordingSelectionKey(entry.kind==='result'?'cleanup-result':'cleanup-recording',entry.id))} onChecked={checked=>historySelected=toggleRecordingSelection(historySelected,entry.kind==='result'?'cleanup-result':'cleanup-recording',entry.id,checked)} selectionLabel={`Select ${entry.kind==='result'?'cleaned':'original'} recording: ${entry.name}`} aria-label={`Open ${entry.kind==='result'?'cleaned':'original'} recording: ${entry.name}`} data-cleanup-kind={entry.kind} data-cleanup-id={entry.id} onSelect={()=>selectHistory(entry)}/>
      {/each}</div>
    {/snippet}
    <div class="cleanup-content task-panel" data-card-scroller>
  <div class="cleanup-model"><div class="cleanup-icon" aria-hidden="true">✦</div><div><span>ON YOUR COMPUTER</span><h1>Voice Cleanup</h1><p>Reduce noise and restore speech while keeping your original.</p></div><span class="cleanup-badge">{status.state==='ready'?'Ready':status.state==='checking'?'Checking…':'Setup needed'}</span></div>
  <div class="cleanup-status" role="status"><p>{restoring?'Opening your cleanup task…':active?'Your recording is being cleaned in the background.':status.state==='ready'?'Ready to clean up your recording.':status.message}</p>{#if status.state!=='ready'}<button class="secondary" onclick={refreshStatus} disabled={!desktop||status.state==='checking'}>Check voice tools again</button>{/if}</div>
  <RecordingInput target="cleanup" onReceive={receiveRecording} disabled={busy||active||deleteBusy} collapseWhenDisabled disabledReason={active?'Cleanup is running. You can play your recording or cancel cleanup.':''}/>
  <p class="cleanup-caution">Start with a short recording. Longer recordings can take a while.</p>
  <div class="cleanup-steps"><span>01 · Import recording</span><span>02 · Clean up</span><span>03 · Compare &amp; save</span></div>
  <input class="visually-hidden" type="file" accept=".wav,.mp3,.flac,.ogg,.m4a" aria-label="Choose recording for Voice Cleanup" bind:this={input} disabled={!desktop||restoring||busy||active||deleteBusy} onchange={()=>choose(input?.files?.[0])}/>
  {#if error}<p class="transcription-error" role="alert">{error}</p>{/if}{#if message}<p class="cleanup-message" role="status">{message}</p>{/if}
  {#if job}<div class="cleanup-job" role="status"><span>{job.stage||job.state}</span>{#if active}<div class="cleanup-progress" role="progressbar" aria-label="Voice cleanup in progress" aria-valuemin="0" aria-valuemax="100" aria-valuenow={job.progress??undefined} aria-valuetext={job.progress==null?job.stage:`${job.progress}%`}><span class:indeterminate={job.progress==null} style:width={job.progress==null?'35%':`${job.progress}%`}></span></div><button class="secondary" onclick={cancel}>Cancel cleanup</button>{/if}</div>{/if}
  {#snippet activeCleanup()}<div class="active-cleanup">
  <div class="cleanup-panels">
    <RecordingPanel title="Original recording">{#snippet actions()}{#if result||imported}<label class="cleanup-card-select"><input type="checkbox" aria-label="Select current cleanup recording" disabled={restoring||busy||active||deleteBusy} checked={historySelected.includes(recordingSelectionKey(result?'cleanup-result':'cleanup-recording',result?.id??imported?.id??''))} onchange={event=>historySelected=toggleRecordingSelection(historySelected,result?'cleanup-result':'cleanup-recording',result?.id??imported?.id??'',event.currentTarget.checked)}/>Select recording</label>{/if}{#if originalUrl}<button class="secondary" disabled={busy||active||deleteBusy} onclick={()=>input?.click()}>Replace</button>{/if}{/snippet}
      {#if originalUrl||imported}<p class="cleanup-filename">{result?.name??imported?.name}</p>{#if originalUrl}<WaveformPlayer src={originalUrl} label={`Original: ${result?.name??imported?.name??'recording'}`} id="cleanup-original" regions={recordingRegions(sourceMedia)}/>{:else}<p class="cleanup-note">{busy||restoring?'Opening your recording…':'Playback could not be opened.'}</p>{#if !busy&&!restoring&&imported}<button class="secondary" onclick={()=>imported&&openRecording(imported.id,true)}>Retry opening recording</button>{/if}{/if}<div class="cleanup-file-meta"><span>{active?'Cleaning up · ':''}Saved locally · Original preserved</span></div>
      {:else}<div class="cleanup-drop" role="region" aria-label="Drop a recording" ondragover={event=>event.preventDefault()} ondrop={dropRecording}><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 16V3m-4 4 4-4 4 4M4 15v6h16v-6"/></svg><h3>Start with a recording</h3><p>WAV, MP3, FLAC, OGG, M4A · Mono/stereo · 8–48 kHz · Up to 100 MB</p><button class="secondary" disabled={!desktop||restoring||busy||active||deleteBusy} onclick={()=>input?.click()}>{busy?'Opening…':'Import recording'}</button></div>{/if}
    </RecordingPanel>
    <RecordingPanel title="Cleaned recording">{#snippet actions()}<span class="cleanup-badge">{cleanedUrl?'Saved result':'Separate output'}</span>{/snippet}{#if cleanedUrl&&result}<p class="cleanup-filename">{result.name}</p><WaveformPlayer src={cleanedUrl} label={`Cleaned: ${result.name}`} id="cleanup-cleaned" regions={recordingRegions(sourceMedia)}/>{#if result.sample_rate}<p class="cleanup-note">{(result.sample_rate/1000).toFixed(1)} kHz · {result.channels===2?'Stereo':'Mono'}{#if result.frames} · {(result.frames/result.sample_rate).toFixed(1)} seconds{/if}</p>{/if}<button class="secondary cleanup-export" onclick={exportResult} disabled={exporting}>{exporting?'Exporting…':'Export cleaned WAV'}</button>{:else}<div class="cleanup-output-empty"><span aria-hidden="true">≋</span><h3>A clearer version, alongside the original</h3><p>{active?'Cleanup is working in the background. You can cancel without losing your original.':'Import a recording and start cleanup. The finished version will appear here for comparison.'}</p></div>{/if}</RecordingPanel>
  </div>
  {#if cleanedUrl&&result}<RecordingActions source={{kind:'cleanup',id:result.id}} label="cleaned recording" owner="cleanup" disabled={busy||active||deleteBusy} copyPath/>{:else if imported}<RecordingActions source={{kind:'cleanup-recording',id:imported.id}} label="cleanup recording" owner="cleanup" disabled={busy||active||deleteBusy} copyPath/>{/if}
  <div class="cleanup-action"><div><h2>Keep the original. Compare the result.</h2><p>Cleanup creates a separate recording. Listen to both versions before deciding which to use.</p></div><button onclick={start} disabled={!imported||busy||active||deleteBusy||status.state!=='ready'}>{active?'Cleaning up…':'Clean up voice'}</button></div>
  </div>{/snippet}
  {#if historyEntries.length}<button class="secondary cleanup-import" disabled={!desktop||restoring||busy||active||deleteBusy} onclick={()=>input?.click()}>Import recording</button>{/if}
  <div class="cleanup-card-list">
  {#each historyEntries as entry (`${entry.kind}-${entry.id}`)}
    <WorkspaceCard id={workspaceCardId(`cleanup-${entry.kind}`,entry.id)} name={entry.name} rename={{kind:entry.kind==='result'?'cleanup-result':'cleanup-recording',id:entry.id}} detail={entry.kind==='result'?'Cleaned recording':active&&entry.id===job?.audioId?'Cleaning…':'Original recording'} expanded={currentEntry(entry)&&!cardCollapsed} disabled={restoring||busy||active||deleteBusy} checked={historySelected.includes(recordingSelectionKey(entry.kind==='result'?'cleanup-result':'cleanup-recording',entry.id))} onChecked={checked=>historySelected=toggleRecordingSelection(historySelected,entry.kind==='result'?'cleanup-result':'cleanup-recording',entry.id,checked)} onOpen={()=>toggleCard(entry)}>
      {@render activeCleanup()}
    </WorkspaceCard>
  {:else}{@render activeCleanup()}{/each}
  </div>
  <p class="cleanup-note">Audio stays on this computer. Saved originals and results remain available after reopening the app. Exports go to Downloads / EthiopicVoiceStudio.</p>
    </div>
  </HistoryPane>
</section>
<style>
  .cleanup-card-list,.active-cleanup{display:flex;flex-direction:column;gap:18px;min-width:0}.cleanup-import{font-size:12px;margin-bottom:18px}
  .cleanup-card-select{display:flex;align-items:center;gap:7px;font-size:11px;color:var(--studio-muted)}.cleanup-card-select input{width:16px;height:16px;accent-color:var(--studio-accent)}
  .cleanup-history-tools{display:flex;align-items:center;justify-content:space-between;gap:10px;padding:0 18px 12px;flex:none;color:var(--studio-muted);font-size:11px}.cleanup-history-tools button{font-size:10px;padding:6px 9px}.cleanup-history-list{display:flex;flex-direction:column;gap:6px}.cleanup-history-empty,.cleanup-history-error{padding:8px;font-size:12px;line-height:1.7;color:var(--studio-muted)}.cleanup-history-error{margin:0 18px 12px;padding:0;flex:none;color:var(--studio-tone-ffb2be)}
  .cleanup-progress{flex:1;min-width:120px;height:9px;overflow:hidden;border-radius:8px;background:var(--studio-tone-28334a)}.cleanup-progress>span{display:block;height:100%;border-radius:inherit;background:var(--studio-accent);transition:width .15s linear}.cleanup-progress>span.indeterminate{animation:cleanup-pending 1.4s ease-in-out infinite alternate}@keyframes cleanup-pending{from{transform:translateX(-100%)}to{transform:translateX(285%)}}@media(prefers-reduced-motion:reduce){.cleanup-progress>span{transition:none}.cleanup-progress>span.indeterminate{animation:none;transform:translateX(90%)}}
  .cleanup-status{display:flex;align-items:center;gap:12px;flex-wrap:wrap;margin-top:16px;color:var(--studio-muted);font-size:12px}.cleanup-status p{margin:0}.cleanup-status button{font-size:11px;padding:7px 12px}.cleanup-caution{font-size:12px;line-height:1.8;color:var(--studio-tone-efd291);border-left:2px solid var(--studio-gold);padding-left:12px;max-width:750px}.cleanup-message{font-size:12px;color:var(--studio-tone-83e2b7);line-height:1.8}.cleanup-job{display:flex;align-items:center;gap:14px;flex-wrap:wrap;padding:12px 0 18px;font-size:12px;color:var(--studio-tone-b8c3db)}.cleanup-job button,.cleanup-export{font-size:11px;padding:9px 14px}.cleanup-export{margin-top:15px}

  .cleanup-workspace{display:flex;flex:1;min-height:0;min-width:0;overflow:hidden}.cleanup-content{padding:28px 32px;overflow-y:auto;flex:1;min-height:0;min-width:0}.cleanup-model{display:flex;align-items:center;gap:14px;padding-bottom:24px;border-bottom:1px solid var(--studio-divider)}.cleanup-icon{width:46px;height:46px;display:grid;place-items:center;background:var(--studio-tone-142e32);color:var(--studio-accent);border-radius:15px;font-size:24px;flex-shrink:0}.cleanup-model>div:not(.cleanup-icon){min-width:0}.cleanup-model>div>span{font-size:9px;color:var(--studio-muted);letter-spacing:1.5px}.cleanup-model h1{overflow-wrap:anywhere;font-size:19px;margin:3px 0 5px;font-weight:600}.cleanup-model p{font-size:12px;color:var(--studio-muted);margin:0}.cleanup-badge{font-size:9px;color:var(--studio-tone-a9b7ce);white-space:nowrap;border:1px solid var(--studio-border);border-radius:12px;padding:4px 9px}.cleanup-model>.cleanup-badge{margin-left:auto}.cleanup-steps{display:flex;flex-wrap:wrap;gap:18px;margin:22px 0;color:var(--studio-muted);font-size:11px}.cleanup-steps span:first-child{color:var(--studio-tone-7be3bd)}.cleanup-panels{display:flex;flex-direction:column;gap:18px}.cleanup-filename{font-size:12px;color:var(--studio-tone-b8c3db);overflow-wrap:anywhere;margin:0 0 10px}.cleanup-file-meta{display:flex;align-items:center;justify-content:space-between;gap:12px;flex-wrap:wrap;margin-top:14px;color:var(--studio-muted);font-size:10px}.cleanup-drop{border:1px dashed var(--studio-border);border-radius:12px;min-height:190px;display:flex;flex-direction:column;align-items:center;justify-content:center;text-align:center;padding:22px}.cleanup-drop svg{width:27px;height:27px;fill:none;stroke:var(--studio-accent);stroke-width:1.5;stroke-linecap:round;stroke-linejoin:round}.cleanup-drop h3,.cleanup-output-empty h3{font-size:14px;font-weight:500;margin:13px 0 7px}.cleanup-drop p,.cleanup-output-empty p{font-size:11px;color:var(--studio-muted);line-height:1.8;max-width:360px;margin:0 0 16px}.cleanup-drop button{font-size:11px;padding:9px 15px}.cleanup-output-empty{display:flex;flex-direction:column;align-items:center;text-align:center;padding:15px 10px}.cleanup-output-empty>span{font-size:30px;color:var(--studio-tone-526580)}.cleanup-action{display:flex;align-items:center;justify-content:space-between;gap:18px;border:1px solid var(--studio-border);border-radius:14px;background:var(--studio-panel);padding:18px 20px;margin-top:22px}.cleanup-action h2{font-size:13px;font-weight:500;margin:0 0 5px}.cleanup-action p{font-size:11px;line-height:1.8;color:var(--studio-muted);max-width:360px;margin:0}.cleanup-action button{font-size:12px;flex-shrink:0}.cleanup-note{font-size:10px;color:var(--studio-tone-8595b2);line-height:1.8;margin:18px 0 0}@media(min-width:1350px){.cleanup-panels{display:grid;grid-template-columns:1fr 1fr}}@media(max-width:800px){.cleanup-content{padding:22px}.cleanup-model{flex-wrap:wrap}.cleanup-model h1{overflow-wrap:anywhere;font-size:17px}.cleanup-model>.cleanup-badge{margin-left:0}.cleanup-action{flex-direction:column;align-items:stretch}}
</style>
