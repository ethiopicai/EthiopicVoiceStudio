<script lang="ts">
  import {reviewDownload} from './download-review.js';
  import {onMount,tick} from 'svelte';
  import {invoke} from '@tauri-apps/api/core';
  import WaveformPlayer from './WaveformPlayer.svelte';
  import {recordingTitles,displayRecordingTitle,forgetRecordingTitles,refreshRecordingTitles} from './recording-titles.js';
  import HistoryPane from './HistoryPane.svelte';
  import HistoryEntry from './HistoryEntry.svelte';
  import WorkspaceCard from './WorkspaceCard.svelte';
  import {scrollToWorkspaceCard,workspaceCardId} from './card-navigation.js';
  import RecordingPanel from './RecordingPanel.svelte';
  import HistorySelection from './HistorySelection.svelte';
  import {recordingSelectionKey,toggleRecordingSelection} from './recording-selection.js';
  import type {HistoryDeleteItem,HistoryDeleteResult} from './history-selection-types';
  import TimedTranscript from './TimedTranscript.svelte';
  import PlaybackHighlightControl from './PlaybackHighlightControl.svelte';
  import RecordingInput from './RecordingInput.svelte';
  import {readRecordingDrop,requestRecordingTransfer,resolveRecording} from './recording-transfer.js';
  import RecordingActions from './RecordingActions.svelte';
  import {editingBlocker,wordDiff} from './voice-editing.js';
  import SpeechTextCheck from './SpeechTextCheck.svelte';
  import {prepareSpeechText} from './speech-text.js';
  function projectTitle(item:{id:string;name:string}){return displayRecordingTitle($recordingTitles,'voice-project',item.id,item.name);}
  type Word={word:string;start:number;end:number;aligned?:boolean};
  type Region={span_in:number[];span_out:number[];seed?:number};
  type Version={audio:string;text:string;words:Word[];regions:Region[];parent:number|null;kind:string;seed?:number};
  type Project={id:string;name:string;created:number;revision:number;head:number;duration:number;channels:number;versions:Version[]};
  type Preview={text:string;words:Word[];regions:Region[];edits:Region[];seed:number;duration:number};
  type Status={selection:{available:boolean;opt_in_required:boolean;slow:boolean;reason:string;tier:string;download_bytes:number;licenses:string[]};consent:boolean;optIn:boolean;ready:boolean;runtimePresent:boolean;busy:boolean;stage:string;loaded:boolean};
  let {onSetup}:{onSetup:()=>void}=$props();
  const desktop='__TAURI_INTERNALS__' in window;
  let status=$state<Status|null>(null),projects=$state<Project[]>([]),project=$state<Project|null>(null),preview=$state<Preview|null>(null);
  let draft=$state(''),mode=$state<'fix'|'edit'>('fix'),error=$state(''),notice=$state(''),busy=$state(false),highlight=$state(true);
  let leftUrl=$state(''),rightUrl=$state(''),position=$state(0),playing=$state(false),previewPosition=$state(0),previewPlaying=$state(false);
  let left=$state<WaveformPlayer>(),right=$state<WaveformPlayer>(),fileInput=$state<HTMLInputElement>();
  let importTranscript=$state(''),selectedSaved=$state(''),actionError=$state('');
  let consentButton=$state<HTMLButtonElement>();
  let saved=$state<{id:string;name:string;text:string;words:{original:string;corrected:string;start:number;end:number}[]}[]>([]);
  let cardCollapsed=$state(false);
  let historyLoading=$state(false),historyError=$state('');
  let historySelected=$state<string[]>([]),deleteBusy=$state(false);
  let alive=true,selection=0,noticeTimer:ReturnType<typeof setTimeout>;
  const current=$derived(project?.versions[project.head]??null);
  const locked=$derived(busy||!!status?.busy||deleteBusy);
  const canImport=$derived(!!status?.consent&&!locked);
  const historyProjects=$derived((project&&!projects.some(item=>item.id===project?.id)?[project,...projects]:[...projects]).sort((a,b)=>b.created-a.created));
  const selectionEntries=$derived(historyProjects.map(item=>({kind:'voice-project',id:item.id,name:projectTitle(item),revision:item.revision})));
  const preparedText=$derived(prepareSpeechText(draft,{allowEmpty:true}));
  const changes=$derived(wordDiff(current?.text??'',mode==='edit'?preparedText.text:draft));
  const workBlocker=$derived(editingBlocker(status,{busy:locked}));
  const regenerationBlocker=$derived(editingBlocker(status,{busy:locked,regenerate:true,before:current?.text??'',after:preparedText.text,hasWords:!!current?.words.length})||(preparedText.issues.length?{code:'text-check',message:preparedText.issues[0]}:null));
  const actionBlocker=$derived(mode==='edit'?regenerationBlocker:workBlocker);
  const canWork=$derived(!workBlocker);
  const tokens=$derived((current?.words??[]).map((w,i)=>({id:i,text:w.word,start:w.start,end:w.end,changed:current?.regions.some(r=>w.start<r.span_out[1]&&w.end>r.span_out[0])})));
  const previewTokens=$derived((preview?.words??[]).map((w,i)=>({id:i,text:w.word,start:w.start,end:w.end,changed:preview?.regions.some(r=>w.start<r.span_out[1]&&w.end>r.span_out[0])})));
  function flash(text:string){notice=text;clearTimeout(noticeTimer);noticeTimer=setTimeout(()=>notice='',4500);}
  function revoke(){left?.stop();right?.stop();if(leftUrl)URL.revokeObjectURL(leftUrl);if(rightUrl)URL.revokeObjectURL(rightUrl);leftUrl='';rightUrl='';}
  async function refresh(){if(!desktop)return;try{const next=await invoke<Status>('get_voice_edit_status');if(alive)status=next;}catch{if(alive)error='Could not check voice tools. Reopen Voice Editing to retry.';}}
  async function list(){const next=await invoke<Project[]>('list_voice_edit_projects');if(alive)projects=next;}
  async function refreshHistory(){void refreshRecordingTitles();if(!desktop||historyLoading)return;historyLoading=true;historyError='';const updates=await Promise.allSettled([list(),invoke<typeof saved>('list_transcripts').then(value=>{if(alive)saved=value;})]);if(alive){historyLoading=false;if(updates.some(update=>update.status==='rejected'))historyError='Some saved recordings could not be listed. Your projects are unchanged. Try Refresh again.';}}
  async function deleteHistory(items:HistoryDeleteItem[]):Promise<HistoryDeleteResult>{const deleted=await invoke<HistoryDeleteResult>('delete_voice_history',{items});forgetRecordingTitles(deleted.deleted);if(!alive)return deleted;const removed=new Set(deleted.deleted.map(item=>item.id));if(project&&removed.has(project.id)){selection++;revoke();project=null;preview=null;draft='';importTranscript='';position=0;playing=false;notice='';error='';}projects=projects.filter(item=>!removed.has(item.id));return deleted;}
  async function audio(id:string,kind:string){const value=await invoke<{bytes:number[]}>('get_voice_edit_audio',{projectId:id,kind});return URL.createObjectURL(new Blob([new Uint8Array(value.bytes)],{type:'audio/wav'}));}
  async function open(p:Project){const token=++selection;revoke();actionError='';cardCollapsed=false;project=p;preview=null;draft=p.versions[p.head].text;position=0;playing=false;const url=await audio(p.id,'current');if(alive&&token===selection)leftUrl=url;else URL.revokeObjectURL(url);}
  async function pick(p:Project){if(locked)return;if(project?.id===p.id&&leftUrl){cardCollapsed=false;await tick();if(alive)scrollToWorkspaceCard(workspaceCardId('editing',p.id));return;}busy=true;error='';try{await open(p);}catch(e){if(alive)error=String(e);}finally{if(alive)busy=false;}await tick();if(alive)scrollToWorkspaceCard(workspaceCardId('editing',p.id));}
  function toggleCard(p:Project){if(locked)return;if(project?.id===p.id&&!cardCollapsed){cardCollapsed=true;left?.stop();right?.stop();}else if(project?.id===p.id){cardCollapsed=false;}else void pick(p);}
  async function accept(){try{await invoke('accept_voice_edit_consent');await refresh();}catch(e){error=String(e);}}
  async function slow(enabled:boolean){try{await invoke('set_voice_edit_opt_in',{enabled});await refresh();}catch(e){error=String(e);}}
  async function setup(){const approvedPlan=await reviewDownload('editing');if(!approvedPlan)return;busy=true;error='';try{await invoke('prepare_voice_edit',{approvedPlan});await refresh();if(!status?.runtimePresent)flash('Models downloaded. A compatible private voice runtime is still needed.');}catch(e){error=String(e);}finally{busy=false;await refresh();}}
  async function importFile(file?:File,text='',words:Word[]=[]){if(!file||!canImport)return;busy=true;error='';try{const p=await invoke<Project>('import_voice_edit_project',{name:file.name,bytes:Array.from(new Uint8Array(await file.arrayBuffer())),text,words});if(!alive)return;await open(p);await list();}catch(e){if(alive)error=String(e);}finally{if(alive){busy=false;if(fileInput)fileInput.value='';}}}
  async function receiveRecording(value:import('./recording-transfer.js').RecordingPackage){if(locked)throw Error('Finish the current editing task before importing another recording.');if(!status)await refresh();if(!status?.consent)throw Error('Accept the recording-rights note above, then retry this import.');busy=true;error='';try{const p=await invoke<Project>('import_voice_edit_project',{name:value.name,bytes:Array.from(value.bytes),text:value.context.text,words:value.context.timingsCurrent?value.context.words:[],sourceMedia:value.context});if(!alive)return;await open(p);await list();}finally{if(alive)busy=false;}}
  async function importSaved(){const t=saved.find(t=>t.id===selectedSaved);if(!t||!canImport)return;error='';try{await receiveRecording(await resolveRecording({kind:'transcript',id:t.id}));}catch(e){if(alive)error=String(e);}}
  async function action(kind:string,seed?:number){if(!project||locked||kind==='preview'&&preparedText.issues.length)return;busy=true;error='';actionError='';const id=project.id,revision=project.revision,originalDraft=draft,alignOriginal=kind==='align'&&mode==='edit',submittedText=alignOriginal?current?.text??'':kind==='preview'?preparedText.text:draft;try{
    const value=await invoke<Project|Preview>('voice_edit_action',{action:kind,projectId:id,revision,text:submittedText,seed:seed??0,referenceText:null});if(!alive||project?.id!==id)return;
    if(kind==='preview'){draft=submittedText;preview=value as Preview;if(rightUrl)URL.revokeObjectURL(rightUrl);const url=await audio(id,'preview');if(!alive){URL.revokeObjectURL(url);return;}rightUrl=url;previewPosition=0;previewPlaying=false;}
    else{await open(value as Project);if(alignOriginal)draft=originalDraft;await list();flash(kind==='fix'?'Transcript fixed. Audio is unchanged.':kind==='keep'?'Edit kept. Changed regions remain marked.':kind==='undo'?'Earlier revision restored.':'Words aligned to this recording.');}
  }catch(e){if(alive)actionError=String(e);}finally{if(alive){busy=false;await refresh();}}}
  async function exportProject(){if(!project||locked)return;busy=true;error='';try{const destination=await invoke<string>('export_voice_edit_project',{projectId:project.id});if(alive)flash(`Exported WAV and edit-log.json to ${destination}`);}catch(e){if(alive)error=String(e);}finally{if(alive)busy=false;}}
  async function cancel(){try{await invoke('cancel_voice_edit');}catch{if(alive)error='The editing task could not be cancelled yet. Please try again.';}}
  function invalidate(){actionError='';preview=null;right?.stop();if(rightUrl)URL.revokeObjectURL(rightUrl);rightUrl='';}
  onMount(()=>{if(desktop){void refresh();void refreshHistory();}const timer=setInterval(()=>void refresh(),1500);return()=>{alive=false;selection++;clearInterval(timer);clearTimeout(noticeTimer);revoke();};});
  function dropRecording(event:DragEvent){if(event.target instanceof Element&&event.target.closest('textarea,input')&&!Array.from(event.dataTransfer?.types??[]).some(type=>type==='application/x-ethiopic-studio-recording'||type==='Files'))return;event.preventDefault();event.stopPropagation();if(locked)return;try{requestRecordingTransfer('infilling',readRecordingDrop(event.dataTransfer));}catch(cause){error=cause instanceof Error?cause.message:'Drop a recording card or audio file.';}}
</script>

<section class="editing-workspace" aria-label="Voice Editing workspace" ondragover={event=>{event.preventDefault();if(locked&&event.dataTransfer)event.dataTransfer.dropEffect='none';}} ondrop={dropRecording}>
  <HistoryPane id="editing-history" label="Editing history" storageKey="ethiopic.editing">
    {#snippet controls()}
      <div class="edit-history-tools"><span>{historyProjects.length} saved</span><button class="secondary" disabled={locked||historyLoading} onclick={refreshHistory} aria-label="Refresh editing history">{historyLoading?'Loading…':'Refresh'}</button></div>
      <HistorySelection entries={selectionEntries} bind:selected={historySelected} bind:busy={deleteBusy} disabled={busy||!!status?.busy} onDelete={deleteHistory}/>
      {#if historyError}<p class="edit-history-error" role="alert">{historyError}</p>{/if}
    {/snippet}
    {#snippet history()}
      {#if historyLoading&&!historyProjects.length}<p class="edit-history-empty" role="status">Loading recordings…</p>{:else if !historyProjects.length}<p class="edit-history-empty">No editing projects yet. Import a recording to get started.</p>{/if}
      <div class="edit-history-list">{#each historyProjects as item (item.id)}<HistoryEntry name={projectTitle(item)} detail={`${item.duration.toFixed(1)} seconds · ${item.versions.length} saved ${item.versions.length===1?'revision':'revisions'}`} selected={project?.id===item.id} disabled={locked} checked={historySelected.includes(recordingSelectionKey('voice-project',item.id))} onChecked={checked=>historySelected=toggleRecordingSelection(historySelected,'voice-project',item.id,checked)} selectionLabel={`Select editing recording: ${projectTitle(item)}`} aria-label={`Open editing recording: ${projectTitle(item)}`} data-editing-id={item.id} onSelect={()=>pick(item)}/>{/each}</div>
    {/snippet}
    <div class="editing-content task-panel" data-card-scroller>
      <div class="edit-heading"><div class="edit-icon" aria-hidden="true"><svg viewBox="0 0 24 24"><path d="M3 10v4m4-7v10m4-13v16m5-9h6m-3-3v6"/></svg></div><div><span>ON YOUR COMPUTER</span><h1>Voice Editing</h1><p>Fix the text, or change a phrase in your recording.</p></div><button class="secondary" onclick={onSetup}>Voice tools</button></div>
      {#if !desktop}<p class="notice">Open the desktop app to edit recordings locally.</p>{/if}
      {#if status}
        {#if !status.consent}<div class="edit-consent" id="editing-consent"><p>Only edit recordings you have the right to change. Edits must not misrepresent what someone said.</p><p>Studio is licensed for non-commercial use; model licenses are separate. Changed regions are marked and included in the export log.</p><button bind:this={consentButton} onclick={accept}>I understand</button></div>{/if}
        {#if status.selection.opt_in_required||status.selection.slow}<label class="slow-choice"><input type="checkbox" checked={status.optIn} disabled={locked} onchange={e=>slow(e.currentTarget.checked)}/> Enable slow editing · about 15–20 seconds per change, depending on this computer</label>{/if}
        <p class="edit-availability" role="status">{status.selection.reason}</p>
        {#if status.selection.available&&!status.ready}<div class="edit-setup"><p>Download voice tools when you need them. Interrupted downloads resume.</p><span>Up to {(status.selection.download_bytes/1e9).toFixed(2)} GB · Cached files are reused</span><button disabled={locked||!status.consent} onclick={setup}>Prepare Voice Editing</button>{#if !status.runtimePresent}<p>Finish voice tool setup in Settings to enable editing.</p>{/if}</div>{/if}
      {/if}
      {#if locked}<div class="edit-progress" role="status"><span class="edit-pulse" aria-hidden="true"></span><span>{status?.stage||'Preparing your recording…'}</span><button class="secondary" onclick={cancel}>Stop</button></div>{/if}
      {#if error}<p class="notice" role="alert">{error}</p>{/if}{#if notice}<p class="edit-notice" role="status">{notice}</p>{/if}
      <RecordingInput target="infilling" onReceive={receiveRecording} disabled={locked} collapseWhenDisabled/>
      <div class="edit-import-tools"><button class="secondary" disabled={!canImport} onclick={()=>fileInput?.click()}>Import audio</button><input type="file" class="visually-hidden" accept=".wav,.mp3,.flac,.ogg,.m4a" aria-label="Choose recording for Voice Editing" bind:this={fileInput} disabled={!canImport} onchange={e=>importFile(e.currentTarget.files?.[0],importTranscript)}/>
        <details><summary>Existing transcript for audio import (optional)</summary><textarea class="import-transcript" aria-label="Existing transcript for audio import" bind:value={importTranscript} disabled={locked} lang="am" rows="3"></textarea></details><details><summary>Import a saved transcript</summary><div class="saved-import"><label for="editing-saved">Recording</label><select id="editing-saved" bind:value={selectedSaved} disabled={!canImport}><option value="">Choose recording</option>{#each saved as item}<option value={item.id}>{item.name}</option>{/each}</select><button class="secondary" disabled={!selectedSaved||!canImport} onclick={importSaved}>Copy into editing project</button></div></details>
      </div>
      <div class="edit-main">
      {#snippet activeEditor()}{#if project&&current}<div class="active-editor">
        <div class="edit-project-heading"><div><h2>{projectTitle(project)}</h2><p>Saved locally · {project.versions.length} saved {project.versions.length===1?'revision':'revisions'}</p></div><div class="edit-project-actions"><button class="secondary" disabled={locked||current.parent===null} onclick={()=>action('undo')}>Undo</button><button class="secondary" disabled={locked} onclick={exportProject}>Export WAV &amp; edit log</button></div></div>
        {#if project.channels>1}<p class="edit-hint">Regenerated regions use the same mono speech in each channel. Original channels outside those regions are preserved.</p>{/if}
        <div class="edit-players" class:has-preview={!!preview}>
          <RecordingPanel title={project.head===0?'Original recording':'Current recording'} label="Current recording">
            {#snippet actions()}<label class="edit-card-select"><input type="checkbox" aria-label="Select current editing recording" disabled={locked} checked={historySelected.includes(recordingSelectionKey('voice-project',project?.id??''))} onchange={event=>{if(project)historySelected=toggleRecordingSelection(historySelected,'voice-project',project.id,event.currentTarget.checked);}}/>Select recording</label>{/snippet}
            <p class="edit-filename">{projectTitle(project)}</p>
            {#if leftUrl}<WaveformPlayer bind:this={left} src={leftUrl} label="Current recording" id={`editing-${project.id}-current`} regions={[...current.regions.map(r=>({start:r.span_out[0],end:r.span_out[1]})),...(preview?.edits??[]).map(r=>({start:r.span_in[0],end:r.span_in[1]}))]} onPlaybackPosition={(p,r)=>{position=p;playing=r;}}/>{/if}
            <TimedTranscript {tokens} text={current.text} {position} {playing} {highlight} language="am" label="Recording transcript" onSelect={t=>{if(t.end>t.start)void left?.playSpan(t.start,t.end);}}/>
            <PlaybackHighlightControl bind:enabled={highlight}/>
            {#if current.regions.length}<div class="permanent-regions"><strong>Edited regions · Synthetic speech</strong>{#each current.regions as region,i}<button class="secondary" onclick={()=>left?.playSpan(Math.max(0,region.span_out[0]-1.5),Math.min(project?.duration??0,region.span_out[1]+1.5))}>Around edit {i+1} · {region.span_out[0].toFixed(2)}–{region.span_out[1].toFixed(2)} s</button>{/each}</div>{/if}
          </RecordingPanel>
          {#if preview}<RecordingPanel title="Edited preview" label="Edited preview">
            <p class="edit-filename">{projectTitle(project)}</p>
            {#if rightUrl}<WaveformPlayer bind:this={right} src={rightUrl} label="Edited preview" id={`editing-${project.id}-preview`} regions={preview.regions.map(r=>({start:r.span_out[0],end:r.span_out[1]}))} onPlaybackPosition={(p,r)=>{previewPosition=p;previewPlaying=r;}}/>{/if}
            <TimedTranscript tokens={previewTokens} position={previewPosition} playing={previewPlaying} {highlight} language="am" label="Edited transcript" onSelect={t=>{if(t.end>t.start)void right?.playSpan(t.start,t.end);}}/>
            <p class="edit-hint">Synthetic regions are marked. Listen before keeping this edit.</p>
            <div class="edit-around">{#each preview.edits as region,i}<button class="secondary" onclick={()=>left?.playSpan(Math.max(0,region.span_in[0]-1.5),Math.min(project?.duration??0,region.span_in[1]+1.5))}>Before edit {i+1}</button><button class="secondary" onclick={()=>right?.playSpan(Math.max(0,region.span_out[0]-1.5),Math.min(preview?.duration??0,region.span_out[1]+1.5))}>Listen around edit {i+1}</button>{/each}</div>
            <div class="edit-actions"><button disabled={locked} onclick={()=>action('keep')}>Keep this edit</button><button class="secondary" disabled={!canWork} onclick={()=>action('preview',preview!.seed+1)}>Try again</button><small>Take seed {preview.seed}</small></div>
          </RecordingPanel>{/if}
        </div>
        <RecordingActions source={{kind:'editing',id:project.id}} label="edited recording" owner="infilling" disabled={locked} copyPath/>
        <RecordingPanel title="Edit your recording" label="Transcript and speech editing">
          <div class="edit-mode" role="group" aria-label="Editing action"><button class="secondary" class:chosen={mode==='fix'} disabled={locked} onclick={()=>mode='fix'}>Fix transcript</button><button class="secondary" class:chosen={mode==='edit'} disabled={locked} onclick={()=>mode='edit'}>Change what is said</button></div>
          <p class="edit-hint">{mode==='fix'?'Correct what was actually spoken. This saves text and alignment without changing audio.':'Change a short phrase. Only changed regions and their transition seams are regenerated.'}</p>
          <label for="edit-script">{mode==='fix'?'Recording transcript':'New wording'}</label><textarea id="edit-script" lang="am" bind:value={draft} spellcheck="false" disabled={locked} rows="5" oninput={invalidate}></textarea>
          {#if mode==='edit'}<div class="edit-diff" aria-label="Pending word changes">{#each changes as part}{#if part.kind==='delete'}<del>{part.text} </del>{:else if part.kind==='insert'}<ins>{part.text} </ins>{:else}<span>{part.text} </span>{/if}{/each}</div>
            <SpeechTextCheck prepared={preparedText} disabled={locked} onApply={()=>{draft=preparedText.text;invalidate();}}/>
            {#if draft.trim().split(/\s+/).length<current.text.trim().split(/\s+/).length}<p class="edit-hint">Deletion can clip a neighboring word. Listen around the edit; include and retype that neighbor if needed.</p>{/if}
          {/if}
          {#if actionError}<p class="edit-action-error" role="alert">{actionError}</p>{/if}
          {#if actionBlocker}<div class="edit-action-status" id="editing-action-status" role="status"><p>{actionBlocker.message}</p>
            {#if actionBlocker.code==='setup'}<button class="secondary" onclick={setup}>Prepare Voice Editing</button>
            {:else if actionBlocker.code==='slow'}<button class="secondary" onclick={()=>slow(true)}>Enable slow editing</button>
            {:else if actionBlocker.code==='consent'}<button class="secondary" onclick={()=>{scrollToWorkspaceCard('editing-consent');consentButton?.focus({preventScroll:true});}}>Review recording rights</button>
            {:else if actionBlocker.code==='checking'}<button class="secondary" onclick={refresh}>Check voice tools again</button>{/if}
          </div>{/if}
          <div class="edit-actions">{#if mode==='fix'}<button aria-describedby={actionBlocker?'editing-action-status':undefined} disabled={!canWork||!draft.trim()} onclick={()=>action('fix')}>Save transcript only</button>{:else}<button aria-describedby={actionBlocker?'editing-action-status':undefined} title={regenerationBlocker?.message} disabled={!!regenerationBlocker} onclick={()=>action('preview',Math.floor(Math.random()*2147483647))}>{draft.trim()?'Regenerate':'Preview removing all speech'}</button>{/if}<button class="secondary" disabled={!canWork} onclick={()=>action('align')}>Align words</button></div>
        </RecordingPanel>
        <details class="edit-history"><summary>Edit history · {project.versions.length} saved revisions</summary>{#each project.versions as version,i}<p class:history-active={i===project.head}>{i===project.head?'Current · ':''}{version.kind==='edit'?'Voice edit':version.kind==='fix'?'Transcript fix':version.kind==='align'?'Alignment':'Original recording'}{version.seed!=null?` · seed ${version.seed}`:''}</p>{/each}</details>
      </div>{/if}{/snippet}
      {#each historyProjects as item (item.id)}
        <WorkspaceCard id={workspaceCardId('editing',item.id)} name={projectTitle(item)} rename={{kind:'voice-project',id:item.id}} detail={`${item.duration.toFixed(1)} seconds · ${item.versions.length} saved revisions`} expanded={project?.id===item.id&&!cardCollapsed} disabled={locked} checked={historySelected.includes(recordingSelectionKey('voice-project',item.id))} onChecked={checked=>historySelected=toggleRecordingSelection(historySelected,'voice-project',item.id,checked)} onOpen={()=>toggleCard(item)}>
          {#snippet summary()}<p class="card-transcript" lang="am">{item.versions[item.head]?.text||'Open this card to transcribe or edit the recording.'}</p>{/snippet}
          {@render activeEditor()}
        </WorkspaceCard>
      {:else}<RecordingPanel title="Start with a recording" label="Start a voice editing project"><p class="edit-hint">Import audio or copy a saved transcript into an editing project. Your source recording stays unchanged.</p></RecordingPanel>{/each}
      </div>
    </div>
  </HistoryPane>
</section>
<style>
  .edit-action-status{margin-top:16px;padding:12px 14px;border:1px solid var(--studio-border);border-radius:10px;color:var(--studio-muted);font-size:12px;line-height:1.7}.edit-action-status p{margin:0}.edit-action-status button{font-size:11px;margin-top:10px}.edit-action-error{color:var(--studio-tone-ffb2be);font-size:12px;line-height:1.7;white-space:pre-wrap;overflow-wrap:anywhere}
  .active-editor{display:flex;flex-direction:column;gap:18px}.card-transcript{font-size:13px;line-height:1.8;color:var(--studio-muted);white-space:pre-wrap;overflow-wrap:anywhere;margin:14px 0 0;max-height:5.4em;overflow:hidden}.import-transcript{width:100%;box-sizing:border-box;margin-top:12px;background:var(--studio-panel);color:var(--studio-text);border:1px solid var(--studio-border);border-radius:8px;padding:10px}
  .edit-card-select{display:flex;align-items:center;gap:7px;font-size:11px;color:var(--studio-muted)}.edit-card-select input{width:16px;height:16px;accent-color:var(--studio-accent)}
  .editing-workspace{display:flex;flex:1;min-height:0;min-width:0;overflow:hidden}.editing-content{padding:28px 32px;overflow-y:auto;flex:1;min-height:0;min-width:0;container-type:inline-size}.edit-heading{display:flex;align-items:center;gap:14px;padding-bottom:24px;border-bottom:1px solid var(--studio-divider);margin-bottom:16px}.edit-icon{display:grid;place-items:center;flex:none;width:46px;height:46px;background:var(--studio-tone-142e32);color:var(--studio-accent);border-radius:15px}.edit-icon svg{width:26px;height:26px;fill:none;stroke:currentColor;stroke-width:1.6;stroke-linecap:round}.edit-heading>div:not(.edit-icon){min-width:0}.edit-heading>div>span{font-size:9px;color:var(--studio-muted);letter-spacing:1.5px}.edit-heading h1{font-size:19px;font-weight:600;margin:3px 0 5px;overflow-wrap:anywhere}.edit-heading p{font-size:12px;color:var(--studio-muted);margin:0}.edit-heading>button{margin-left:auto;flex:none;font-size:11px;padding:7px 11px}.edit-history-tools{display:flex;align-items:center;justify-content:space-between;gap:10px;padding:0 18px 12px;flex:none;color:var(--studio-muted);font-size:11px}.edit-history-tools button{font-size:10px;padding:6px 9px}.edit-history-list{display:flex;flex-direction:column;gap:6px}.edit-history-empty,.edit-history-error{font-size:12px;line-height:1.7;color:var(--studio-muted);padding:8px}.edit-history-error{padding:0;margin:0 18px 12px;flex:none;color:var(--studio-tone-ffb2be)}.edit-main{display:flex;flex-direction:column;gap:18px;min-width:0}.edit-project-heading{display:flex;align-items:center;justify-content:space-between;gap:14px;flex-wrap:wrap}.edit-project-heading>div{min-width:0}.edit-project-heading h2{font-size:14px;font-weight:600;overflow-wrap:anywhere;margin:0 0 5px}.edit-project-heading p{font-size:10px;color:var(--studio-muted);margin:0}.edit-project-actions{display:flex;gap:8px;flex-wrap:wrap}.edit-project-actions button{font-size:11px;padding:7px 10px}.edit-players{display:grid;grid-template-columns:minmax(0,1fr);gap:18px}.edit-filename{font-size:12px;color:var(--studio-tone-b8c3db);overflow-wrap:anywhere;margin:0 0 10px}.edit-import-tools{display:flex;align-items:center;gap:14px;flex-wrap:wrap;margin:0 0 22px;font-size:12px}.edit-import-tools>button,.saved-import button{font-size:11px;padding:9px 12px}.edit-import-tools details{flex:1;min-width:0}.edit-import-tools summary{cursor:pointer;color:var(--studio-muted)}.saved-import{display:flex;flex-wrap:wrap;gap:8px;align-items:center;margin-top:12px}.saved-import label{font-size:11px;color:var(--studio-muted)}.saved-import select{min-width:0;max-width:100%;flex:1;padding:9px;border:1px solid var(--studio-border);border-radius:8px;background:var(--studio-panel);color:var(--studio-text);font:inherit;font-size:12px}.edit-mode{display:flex;gap:10px;flex-wrap:wrap}.edit-mode button{font-size:12px}.chosen{border-color:var(--studio-accent)!important;background:var(--studio-tone-143c36)!important}.edit-main textarea{display:block;resize:vertical;width:100%;box-sizing:border-box;min-height:140px;background:var(--studio-tone-0b1425);border:1px solid var(--studio-tone-354660);color:inherit;border-radius:10px;padding:14px;font:inherit;line-height:1.8;margin:8px 0 12px}.edit-main label{font-size:12px}.edit-hint,.edit-availability,.edit-setup p{font-size:12px;line-height:1.7;color:var(--studio-muted)}.edit-diff{max-height:180px;overflow:auto;line-height:1.9;padding:14px;border:1px solid var(--studio-tone-2e4056);border-radius:10px;margin-bottom:14px;white-space:pre-wrap}.edit-diff del{color:var(--studio-tone-f4a5ab);background:var(--studio-tone-43242e)}.edit-diff ins{color:var(--studio-tone-89e7c7);background:var(--studio-tone-153a31);text-decoration:none}.edit-consent,.edit-setup{border:1px solid var(--studio-tone-3b4e64);background:var(--studio-tone-0e1c2b);border-radius:12px;padding:16px;margin-bottom:18px;font-size:12px;line-height:1.7}.edit-setup span{display:block;color:var(--studio-muted);margin:8px 0}.slow-choice{display:flex;gap:9px;align-items:center;font-size:12px}.edit-progress{display:flex;align-items:center;gap:12px;padding:12px 0 18px;font-size:12px;color:var(--studio-muted)}.edit-progress>span:not(.edit-pulse){flex:1;min-width:0}.edit-progress button{font-size:11px;padding:9px 12px}.edit-pulse{flex:none;width:10px;height:10px;border-radius:50%;background:var(--studio-accent);animation:editing-pulse 1.5s ease-in-out infinite alternate}.edit-actions{display:flex;align-items:center;gap:10px;flex-wrap:wrap;margin:16px 0 0}.edit-actions button,.edit-around button,.permanent-regions button{font-size:12px}.edit-actions small{color:var(--studio-muted)}.edit-around,.permanent-regions{display:flex;gap:8px;flex-wrap:wrap;font-size:12px;margin-top:14px}.permanent-regions strong{width:100%;color:var(--studio-tone-f3c56b)}.edit-history{border:1px solid var(--studio-tone-29364e);border-radius:14px;background:var(--studio-tone-0d1526);padding:16px 20px;font-size:12px;color:var(--studio-muted)}.edit-history summary{cursor:pointer}.history-active{color:var(--studio-tone-75e5c2)}.edit-notice{font-size:12px;color:var(--studio-tone-7be1bf)}select:focus-visible,textarea:focus-visible,summary:focus-visible{outline:2px solid var(--studio-focus);outline-offset:3px}@container(min-width:760px){.edit-players.has-preview{grid-template-columns:repeat(2,minmax(0,1fr))}}@keyframes editing-pulse{to{opacity:.35}}@media(prefers-reduced-motion:reduce){.edit-pulse{animation:none}}@media(max-width:800px){.editing-content{padding:22px}.edit-heading{flex-wrap:wrap}.edit-heading h1{font-size:17px}.edit-heading>button{margin-left:0}}
</style>
