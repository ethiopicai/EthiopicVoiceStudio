<script lang="ts">
  import { onMount, untrack } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { correctionError, playbackSpan } from './transcript.js';
  import type { TimedWord, Transcript } from './transcript-types';
  import WaveformPlayer from './WaveformPlayer.svelte';
  import TimedTranscript from './TimedTranscript.svelte';
  import PlaybackHighlightControl from './PlaybackHighlightControl.svelte';
  import RecordingActions from './RecordingActions.svelte';
  import RenameRecording from './RenameRecording.svelte';
  import {recordingTitles,displayRecordingTitle} from './recording-titles.js';
  import {recordingRegions} from './recording-transfer.js';
  let sourceMedia=$state<import('./recording-transfer.js').RecordingContext|null>(null);
  let {transcript,expanded,onToggle,onUpdated,onDelete,deleteDisabled=false,number=1,checked=false,selectionDisabled=false,onChecked,onTranscribe}:{transcript:Transcript;expanded:boolean;onToggle:()=>void;onUpdated:(value:Transcript)=>void;onDelete:()=>void;deleteDisabled?:boolean;number?:number;checked?:boolean;selectionDisabled?:boolean;onChecked?:(checked:boolean)=>void;onTranscribe?:()=>void}=$props();
  let mounted=$state(false),player=$state<WaveformPlayer>(),audioUrl=$state(''),loading=$state(false),audioError=$state(''),error=$state('');
  let selected=$state<TimedWord|null>(null),correction=$state(''),selectedRevision=$state<number|null>(null),saving=$state(false),loadAttempt=$state(0);
  let highlightWords=$state(true),playbackPosition=$state(0),playbackRunning=$state(false);
  const audioId=$derived(transcript.id);
  const cardTitle=$derived(displayRecordingTitle($recordingTitles,'transcript',transcript.id,transcript.source_recording_id?displayRecordingTitle($recordingTitles,'recording',transcript.source_recording_id,transcript.name):transcript.name));
  const hasCustomTitle=$derived(!!$recordingTitles[`transcript:${transcript.id}`]||!!(transcript.source_recording_id&&$recordingTitles[`recording:${transcript.source_recording_id}`]));
  const timedTokens=$derived(transcript.words.map(word=>({id:word.id,text:word.corrected||`[${word.word} · removed]`,start:word.start,end:word.end,changed:word.corrected!==word.word})));
  const createdAt=$derived(transcript.created_at_ms&&Number.isFinite(transcript.created_at_ms)?new Date(transcript.created_at_ms):null);
  let actionBusy=$state(false),notice=$state(''),actionError=$state(''),exportMenu=$state<HTMLDetailsElement>();
  let noticeTimer:ReturnType<typeof setTimeout>|undefined;
  function notify(message:string){notice=message;clearTimeout(noticeTimer);noticeTimer=setTimeout(()=>notice='',4000);}
  async function copy(kind:'text'|'path'){
    if(actionBusy||saving)return;actionBusy=true;actionError='';notice='';
    try{
      if(navigator.platform.toLowerCase().includes('mac'))await invoke(kind==='text'?'copy_transcript_text':'copy_transcript_wav_path',{id:transcript.id});
      else{
        const value=kind==='text'?(await invoke<{text:string}>('get_transcript',{id:transcript.id})).text:await invoke<string>('get_transcript_wav_path',{id:transcript.id});
        await navigator.clipboard.writeText(value);
      }
      if(mounted)notify(kind==='text'?'Text copied':'WAV path copied');
    }catch{if(mounted)actionError=kind==='text'?'Text could not be copied. Try again.':'The WAV path could not be copied. Check that the recording is still available.';}
    finally{if(mounted)actionBusy=false;}
  }
  async function exportTimings(format:'vtt'|'srt'|'json'){
    if(actionBusy||saving)return;actionBusy=true;actionError='';notice='';if(exportMenu)exportMenu.open=false;
    try{const output=await invoke<{name:string;message:string}>('export_transcript',{id:transcript.id,format});if(mounted)notify(`Saved ${output.name} to Downloads/EthiopicVoiceStudio`);}
    catch{if(mounted)actionError='Timings could not be exported. Check free storage and try again. Use JSON to preserve all original timing data.';}
    finally{if(mounted)actionBusy=false;}
  }

  function trackPlayback(seconds:number,running:boolean){playbackPosition=seconds;playbackRunning=running;}
  $effect(()=>{
    const id=audioId,enabled=expanded,ready=mounted,attempt=loadAttempt;
    if(!enabled||!ready)return;
    let current=true,url='';loading=true;audioError='';
    void invoke<{name:string;bytes:number[];sourceMedia?:import('./recording-transfer.js').RecordingContext}>('get_transcript_audio',{id}).then(source=>{
      if(!current)return;sourceMedia=source.sourceMedia??null;url=URL.createObjectURL(new Blob([new Uint8Array(source.bytes)],{type:'audio/wav'}));audioUrl=url;
    }).catch(()=>{if(current)audioError='Recording could not be opened. You can still read and correct the transcript.';}).finally(()=>{if(current)loading=false;});
    return()=>{current=false;untrack(()=>player?.stop());if(url)URL.revokeObjectURL(url);audioUrl='';playbackRunning=false;playbackPosition=0;};
  });
  async function pickWord(word:TimedWord){
    selected=word;selectedRevision=transcript.revision;correction=word.corrected;error='';notice='';
    const span=playbackSpan(word);
    if(!span||!player){error='Audio is not ready for this word. You can still correct its text.';return;}
    try{await player.playSpan(span.start,span.end);}catch{if(mounted)error='Word playback is unavailable. Try the full recording; your text is unchanged.';}
  }
  async function save(remove=false){
    if(!selected||saving||actionBusy)return;
    error=remove?'':correctionError(correction);if(error)return;saving=true;notice='';
    try{
      const updated=await invoke<Transcript>(remove?'remove_transcript_word':'correct_transcript_word',{id:transcript.id,wordId:selected.id,revision:selectedRevision,...(remove?{}:{text:correction.trim()})});
      if(!mounted)return;
      onUpdated(updated);selectedRevision=updated.revision;selected=updated.words.find(word=>word.id===selected?.id)??null;correction=selected?.corrected??'';
      notify(remove?'Word removed. Original audio and timing are preserved.':'Correction saved and confirmed by you.');
    }catch{if(mounted)error='This change was not saved. Refresh transcripts and select the word again before retrying.';}finally{if(mounted)saving=false;}
  }
  onMount(()=>{mounted=true;return()=>{mounted=false;clearTimeout(noticeTimer);player?.stop();};});
</script>

<article class="transcript-card recording-card" class:expanded class:batch-selected={checked} id={`transcript-${transcript.id}`} data-transcript-id={transcript.id} aria-label={`Recording ${number}`}>
  <header class="card-header">
    {#if onChecked}<input class="batch-checkbox" type="checkbox" aria-label={`Select recording ${number}`} {checked} disabled={selectionDisabled||saving||actionBusy} onchange={event=>onChecked?.(event.currentTarget.checked)}/>{/if}
    <button class="transcript-card-heading" onclick={onToggle} aria-expanded={expanded} aria-controls={`transcript-body-${transcript.id}`}>
      <span class="recording-label" class:custom-title={hasCustomTitle} title={transcript.name}>{hasCustomTitle?cardTitle:`Recording ${number}`}</span>{#if createdAt}<time datetime={createdAt.toISOString()}>{createdAt.toLocaleString(undefined,{month:'short',day:'numeric',hour:'numeric',minute:'2-digit'})}</time>{/if}{#if !hasCustomTitle}<span class="filename" title={transcript.name}>{transcript.name}</span>{/if}<span class="card-chevron" aria-hidden="true">{expanded?'⌃':'⌄'}</span>
    </button>
    <div class="card-actions"><RenameRecording name={cardTitle} kind="transcript" id={transcript.id} disabled={actionBusy||saving||deleteDisabled}/><button class="card-icon" aria-label={`Copy WAV path for recording ${number}`} title="Copy local WAV path" disabled={actionBusy||saving} onclick={()=>copy('path')}><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M8 8h12v13H8V8Zm-4 8H2V2h12v2M11 12h6m-6 4h6"/></svg></button><button class="card-icon delete-icon" aria-label={`Delete recording ${number}`} title="Delete recording from the app" disabled={actionBusy||saving||deleteDisabled} onclick={onDelete}><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M3 6h18M9 6V3h6v3M6 6l1 15h10l1-15M10 10v7m4-7v7"/></svg></button></div>
  </header>
  <RecordingActions source={{kind:'transcript',id:transcript.id}} label={`recording ${number}`} disabled={saving||actionBusy||deleteDisabled}/>
  {#if expanded}
    <TimedTranscript tokens={timedTokens} text={transcript.text||'No speech recognized.'} position={playbackPosition} playing={playbackRunning} highlight={highlightWords} enabled={expanded} selectedIndex={selected?transcript.words.findIndex(word=>word.id===selected?.id):null} disabled={saving} label={`Transcript text for ${transcript.name}`} language="am" onSelect={(_token,index)=>pickWord(transcript.words[index])}/>

  {:else}<p class="transcript-text collapsed-preview" lang="am">{transcript.text||'No speech recognized.'}</p>{/if}
  <div class="text-actions"><button class="text-action" disabled={actionBusy||saving||!transcript.text} onclick={()=>copy('text')}><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M8 8h12v13H8V8Zm-4 8H2V2h12v2"/></svg>Copy text</button><details class="timing-export" bind:this={exportMenu}><summary>Export timings <span aria-hidden="true">⌄</span></summary><div class="export-options"><button disabled={actionBusy||saving} onclick={()=>exportTimings('vtt')}>WebVTT (.vtt)</button><button disabled={actionBusy||saving} onclick={()=>exportTimings('srt')}>SubRip (.srt)</button><button disabled={actionBusy||saving} onclick={()=>exportTimings('json')}>All word data (.json)</button></div></details></div>
  {#if onTranscribe&&transcript.model_version==='imported'&&!transcript.words.length}<p class="audio-message">Transcript text was copied. Get fresh word timings to follow or correct individual words. <button class="text-action" disabled={actionBusy||saving||deleteDisabled} onclick={onTranscribe}>Get fresh word timings</button></p>{/if}
  {#if notice}<p class="action-notice transcription-confirmation" role="status">{notice}</p>{/if}{#if actionError}<p class="action-error" role="alert">{actionError}</p>{/if}
  <div id={`transcript-body-${transcript.id}`} hidden={!expanded}>
    {#if expanded}
      {#if loading}<p class="audio-message" role="status">Opening recording…</p>{:else if audioError}<p class="audio-message" role="status">{audioError} <button class="retry-audio" onclick={()=>loadAttempt++}>Retry audio</button></p>{:else if audioUrl}<WaveformPlayer bind:this={player} src={audioUrl} label={`Recording ${number}`} id={transcript.id} regions={recordingRegions(sourceMedia)} onPlaybackPosition={trackPlayback}/>{/if}
      {#if sourceMedia?.editedRegions.length}<p class="audio-message">Contains edited or generated speech. Changed regions are marked on the waveform.</p>{/if}
      <div class="playback-options"><PlaybackHighlightControl bind:enabled={highlightWords} disabled={!audioUrl||!transcript.words.length}/><p>Click a word to hear its original span and correct it.</p></div>
      {#if error}<p class="transcription-error" role="alert">{error}</p>{/if}
      {#if selected}<form class="transcript-correction" onsubmit={event=>{event.preventDefault();void save();}}>
        <p>Original: <span lang="am">{selected.word}</span> · {selected.start.toFixed(2)}–{selected.end.toFixed(2)} seconds</p>
        <label for={`correction-${transcript.id}`}>Correct selected word</label><input id={`correction-${transcript.id}`} lang="am" bind:value={correction} maxlength="200" disabled={saving}/>
        <div class="correction-actions"><button type="submit" disabled={saving||actionBusy}>Save correction — I checked this word</button><button class="remove-word" type="button" disabled={saving||actionBusy||selected.corrected===''} onclick={()=>save(true)}>Remove word — I checked</button><button class="close-correction" type="button" disabled={saving} onclick={()=>selected=null}>Close</button></div>
        <p>Edits retain the original audio span; replacement words are not realigned. Draft changes are not saved until confirmed.</p>
      </form>{/if}
    {/if}
  </div>
  {#if !expanded}<button class="play-recording" onclick={onToggle}>▷ Play recording</button>{/if}
</article>

<style>
 .transcript-card-heading:has(.custom-title){flex-wrap:wrap;row-gap:4px}.recording-label.custom-title{white-space:normal;overflow-wrap:anywhere;min-width:0;flex-basis:100%}.custom-title~.card-chevron{margin-left:auto}
 .recording-card.batch-selected{border-color:var(--studio-tone-478c73);box-shadow:inset 0 0 0 1px var(--studio-tone-478c7324)}.batch-checkbox{width:16px;height:16px;flex:none;margin:0;accent-color:var(--studio-accent);cursor:pointer}.batch-checkbox:disabled{opacity:.45;cursor:default}.batch-checkbox:focus-visible{outline:2px solid var(--studio-gold);outline-offset:3px}


 .card-actions{display:flex;align-items:center;gap:3px;margin-left:auto}.card-icon{width:30px;height:30px;display:grid;place-items:center;padding:5px;border-radius:7px;border:0;background:transparent;color:var(--studio-tone-98abc6);cursor:pointer}.card-icon:hover{background:var(--studio-tone-1b2a40);color:var(--studio-tone-e4eefc)}.delete-icon:hover{color:var(--studio-tone-ffb2be);background:var(--studio-tone-352333)}.card-icon svg,.text-action svg{width:17px;height:17px;fill:none;stroke:currentColor;stroke-width:1.6;stroke-linecap:round;stroke-linejoin:round}.text-actions{display:flex;align-items:center;gap:14px;margin:-5px 0 18px;font-size:11px;color:var(--studio-tone-9aafc9)}.text-action{display:flex;align-items:center;gap:6px;font:inherit;font-size:11px;padding:5px 0;background:transparent;border:0;color:var(--studio-tone-9aafc9);font-weight:400;cursor:pointer}.text-action:hover{color:var(--studio-tone-8ce3c4)}.timing-export{position:relative}.timing-export summary{list-style:none;cursor:pointer;padding:5px 0}.export-options{position:absolute;top:calc(100% + 5px);left:0;z-index:5;min-width:170px;padding:5px;background:var(--studio-tone-152138);border:1px solid var(--studio-tone-35465f);border-radius:9px;box-shadow:0 10px 24px var(--studio-tone-0006)}.export-options button{display:block;width:100%;text-align:left;border:0;background:transparent;color:var(--studio-tone-d4e3f5);font:inherit;font-size:11px;font-weight:400;padding:9px 10px;border-radius:5px;cursor:pointer}.export-options button:hover{background:var(--studio-tone-25364d)}.action-notice,.action-error{font-size:11px;line-height:1.6;margin:0 0 12px;overflow-wrap:anywhere}.action-notice{color:var(--studio-tone-86ddbd)}.action-error{color:var(--studio-tone-ffbac7)}.card-icon:focus-visible,.text-action:focus-visible,.timing-export summary:focus-visible,.export-options button:focus-visible{outline:2px solid var(--studio-gold);outline-offset:3px}.card-icon:disabled,.text-action:disabled{opacity:.45;cursor:default}

 .recording-card{padding:20px 22px;border:1px solid var(--studio-tone-26354b);border-radius:14px;background:var(--studio-tone-0d1728);min-width:0}.recording-card .card-header{display:flex;gap:12px;align-items:center;flex-wrap:wrap;color:var(--studio-tone-8fa2bd);font-size:11px}.recording-label{color:var(--studio-tone-c9d6e8);font-weight:600;font-size:12px}.filename{margin-left:auto;max-width:35%;text-overflow:ellipsis;overflow:hidden;white-space:nowrap;color:var(--studio-tone-788ba8)}.transcript-text{font-size:21px;line-height:1.85;color:var(--studio-tone-e9eef9);white-space:pre-wrap;overflow-wrap:anywhere;margin:16px 0 18px}.play-recording{font:inherit;font-size:12px;color:var(--studio-tone-a9bdcf);background:transparent;border:1px solid var(--studio-tone-304057);border-radius:8px;padding:7px 11px;cursor:pointer}.play-recording:hover{color:var(--studio-tone-73dfb7);border-color:var(--studio-tone-487965)}.play-recording:focus-visible{outline:2px solid var(--studio-gold);outline-offset:3px}.audio-message{font-size:12px;color:var(--studio-tone-a5b3c9)}@media(max-width:650px){.recording-card{padding:16px}.filename{display:none}}

 .recording-card{flex-shrink:0;scroll-margin:12px;overflow:visible}.recording-card.expanded{border-color:var(--studio-tone-35524e)}.recording-card .card-header{display:flex;align-items:center;gap:10px;flex-wrap:nowrap;min-width:0}.transcript-card-heading{display:flex;align-items:center;gap:12px;flex:1;min-width:0;padding:0;background:transparent;border:0;color:inherit;font:inherit;font-size:11px;text-align:left;cursor:pointer;font-weight:400}.recording-label{white-space:nowrap}.transcript-card-heading time{white-space:nowrap;color:var(--studio-tone-8fa2bd)}.filename{margin-left:auto;max-width:46%}.card-chevron{color:var(--studio-tone-8fa2bd);font-size:15px}.card-actions{margin-left:0;flex-shrink:0}.recording-card .transcript-text{white-space:normal}.recording-card .collapsed-preview{white-space:pre-wrap;overflow:visible;text-overflow:clip;max-height:none}.playback-options{margin-top:12px;font-size:11px;color:var(--studio-tone-8fa3bd)}.playback-options p{margin:8px 0 0;line-height:1.7}.transcription-error{font-size:12px;line-height:1.6;color:var(--studio-tone-ffbac7);margin-top:12px}.transcript-correction{margin-top:16px;padding:15px;background:var(--studio-tone-111e31);border:1px solid var(--studio-tone-33465f);border-radius:10px;font-size:12px}.transcript-correction p{font-size:11px;line-height:1.7;color:var(--studio-tone-99adc9);margin:0 0 10px}.transcript-correction label{display:block;margin-bottom:7px;color:var(--studio-tone-c5d3e7)}.transcript-correction input{display:block;width:100%;box-sizing:border-box;font:inherit;font-size:17px;padding:10px;border:1px solid var(--studio-tone-435978);border-radius:7px;background:var(--studio-tone-091322);color:var(--studio-tone-e7effa)}.correction-actions{display:flex;gap:8px;flex-wrap:wrap;margin:10px 0}.correction-actions button{font:inherit;font-size:11px;padding:8px 10px;background:var(--studio-tone-1e4b3e);border:1px solid var(--studio-tone-388469);border-radius:7px;color:var(--studio-tone-d2f0e3);font-weight:400;cursor:pointer}.correction-actions .remove-word,.correction-actions .close-correction{background:transparent;border-color:var(--studio-tone-45526b);color:var(--studio-tone-bacce3)}.correction-actions button:disabled{opacity:.5;cursor:default}.retry-audio{background:transparent;border:0;color:var(--studio-tone-87d8bc);font:inherit;font-size:11px;text-decoration:underline;cursor:pointer}.transcript-card-heading:focus-visible,.play-recording:focus-visible,.transcript-correction input:focus-visible,.correction-actions button:focus-visible,.retry-audio:focus-visible{outline:2px solid var(--studio-gold);outline-offset:3px}@media(max-width:650px){.transcript-card-heading{gap:8px}.transcript-card-heading time{font-size:10px}}

</style>
