<script lang="ts">
  import {onMount} from 'svelte';
  import {pendingRecordingTransfer,readRecordingDrop,canDropRecording,resolveRecording} from './recording-transfer.js';
  type Package=import('./recording-transfer.js').RecordingPackage;
  type Input=import('./recording-transfer.js').RecordingInput;
  let {target,onReceive,disabled=false,disabledReason='',collapseWhenDisabled=false}:{target:import('./recording-transfer.js').RecordingTarget;onReceive:(value:Package,input?:Input)=>Promise<void>;disabled?:boolean;disabledReason?:string;collapseWhenDisabled?:boolean}=$props();
  let details=$state<HTMLDetailsElement>();
  let path=$state(''),busy=$state(false),error=$state(''),selected=$state<Pick<Package,'name'|'context'>|null>(null),highlight=$state(false),pending=$state<Input|null>(null);
  let alive=true,request=0,last=0,pendingRequest=0;
  async function receive(input:Input,requestId=0){
    pending=input;pendingRequest=requestId;error='';if(disabled||busy){error='Finish the current task before importing another recording.';return;}
    const token=++request;busy=true;
    try{const value=await resolveRecording(input);if(!alive||token!==request)return;await onReceive(value,input);pendingRecordingTransfer.update(current=>requestId&&current?.id===requestId?null:current);if(!alive||token!==request)return;selected={name:value.name,context:value.context};if(pending===input)pending=null;path='';}
    catch(cause){if(alive&&token===request)error=typeof cause==='string'?cause:cause instanceof Error?cause.message:'This recording could not be opened. Your current work is unchanged.';}
    finally{if(alive&&token===request)busy=false;}
  }
  function drop(event:DragEvent){event.preventDefault();event.stopPropagation();highlight=false;if(disabled||busy)return;try{void receive(readRecordingDrop(event.dataTransfer));}catch(cause){error=String(cause instanceof Error?cause.message:cause);}}
  $effect(()=>{if(disabled||busy)highlight=false;if(disabled&&collapseWhenDisabled&&details)details.open=false;});
  $effect(()=>{const value=$pendingRecordingTransfer;if(value?.target===target&&value.id!==last){last=value.id;void receive(value.input,value.id);}});
  onMount(()=>()=>{alive=false;request++;});
</script>
<section class="recording-input" class:drop-active={highlight} class:input-disabled={disabled||busy} aria-label="Use a recording from another tool" ondragover={event=>{if(canDropRecording(event.dataTransfer)){event.preventDefault();event.stopPropagation();if(disabled||busy){highlight=false;if(event.dataTransfer)event.dataTransfer.dropEffect='none';}else highlight=true;}}} ondragleave={()=>highlight=false} ondrop={drop}>
  <details bind:this={details}><summary aria-disabled={disabled||busy} tabindex={disabled||busy?-1:0} onclick={event=>{if(disabled||busy)event.preventDefault();}}>Use a recording from another tool <span>Drop a card here or paste an audio path</span></summary><form onsubmit={event=>{event.preventDefault();void receive({kind:'path',path});}}><label for={`recording-path-${target}`}>Local audio path</label><div><input id={`recording-path-${target}`} bind:value={path} placeholder="Paste a local audio file path" disabled={busy||disabled} autocomplete="off" spellcheck="false"/><button class="secondary" disabled={busy||disabled||!path.trim()}>Use recording</button></div></form></details>
  {#if disabled&&disabledReason}<p>{disabledReason}</p>{/if}
  {#if busy}<p role="status">Copying your recording and its details…</p>{/if}
  {#if selected}<p role="status">{selected.name} · {selected.context.text?'Transcript and source details copied':'Recording copied'}{selected.context.words.length?' · Word timings included':''}{selected.context.words.length&&!selected.context.timingsCurrent?' · Realign before editing changed speech':''}</p>{/if}
  {#if error}<p class="transfer-error" role="alert">{error}{#if pending}<button class="secondary" disabled={busy||disabled} onclick={()=>pending&&receive(pending,pendingRequest)}>Retry import</button>{/if}</p>{/if}
</section>
<style>
  .input-disabled summary{opacity:.5;cursor:not-allowed}.input-disabled input,.input-disabled button{cursor:not-allowed}
  .recording-input{border:1px dashed var(--studio-border);border-radius:10px;padding:12px 14px;margin:12px 0 18px;font-size:12px;color:var(--studio-muted)}.recording-input.drop-active{border-color:var(--studio-accent);background:var(--studio-tone-12392e)}summary{cursor:pointer;color:var(--studio-text)}summary span{font-size:11px;color:var(--studio-muted);margin-left:8px}form{margin-top:14px}form label{display:block;font-size:11px;margin-bottom:7px}form div{display:flex;gap:8px}input{min-width:0;flex:1;border:1px solid var(--studio-border);border-radius:8px;background:var(--studio-sidebar);color:var(--studio-text);padding:9px;font:inherit}button{font-size:11px;padding:8px 12px}p{font-size:11px;line-height:1.7;margin:9px 0 0}.transfer-error{color:var(--studio-tone-ffb2be)}.transfer-error button{margin-left:10px}@media(max-width:700px){form div{flex-wrap:wrap}input{width:100%}summary span{display:block;margin:5px 0 0}}
</style>
