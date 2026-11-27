<script lang="ts">
  import {invoke} from '@tauri-apps/api/core';
  import {recordingTargets,requestRecordingTransfer,writeRecordingDrag,finishRecordingDrag} from './recording-transfer.js';
  let {source,label='recording',owner='',disabled=false,copyPath=false}:{source:import('./recording-transfer.js').RecordingInput;label?:string;owner?:string;disabled?:boolean;copyPath?:boolean}=$props();
  let menu=$state<HTMLDetailsElement>(),error=$state(''),notice=$state('');
  $effect(()=>{if(disabled&&menu)menu.open=false;});
  function send(target:string){if(disabled)return;if(menu)menu.open=false;requestRecordingTransfer(target as import('./recording-transfer.js').RecordingTarget,source);}
  async function copy(){error='';notice='';try{if(navigator.platform.toLowerCase().includes('mac'))await invoke('copy_recording_transfer_path',{input:source});else{const path=await invoke<string>('get_recording_transfer_path',{input:source});await navigator.clipboard.writeText(path);}notice='WAV path copied';setTimeout(()=>notice='',3500);}catch{error='This WAV path could not be copied. Save the recording in a project first.';}}
</script>
<div class="recording-actions" aria-label={`Transfer ${label}`}>
  <button class="secondary recording-drag" draggable={!disabled} ondragstart={event=>{if(disabled){event.preventDefault();return;}writeRecordingDrag(event,source);}} ondragend={finishRecordingDrag} disabled={disabled} title="Drag recording onto a voice tool in the left menu" aria-label={`Drag ${label} to another voice tool`}>⠿</button>
  <details bind:this={menu}><summary aria-label={`Send ${label} to another tool`} aria-disabled={disabled} tabindex={disabled?-1:0} onclick={event=>{if(disabled)event.preventDefault();}}>Send to <span aria-hidden="true">⌄</span></summary><div class="send-options">{#each recordingTargets.filter(item=>item.id!==owner) as target}<button disabled={disabled} onclick={()=>send(target.id)}>{target.label}</button>{/each}</div></details>
  {#if copyPath&&source.kind!=='file'}<button class="secondary" disabled={disabled} onclick={copy}>Copy WAV path</button>{/if}
  {#if notice}<span role="status">{notice}</span>{/if}{#if error}<span role="alert">{error}</span>{/if}
</div>
<style>
  summary[aria-disabled=true]{opacity:.5;cursor:not-allowed}
  .recording-actions{display:flex;align-items:center;gap:9px;position:relative;flex-wrap:wrap;margin:10px 0;font-size:11px;color:var(--studio-muted)}button{font:inherit;font-size:11px;padding:6px 10px}.recording-drag{font-size:20px;line-height:1;padding:3px 8px;cursor:grab;-webkit-user-drag:element}.recording-drag:active{cursor:grabbing}details{position:relative}summary{cursor:pointer;padding:6px 8px;border:1px solid var(--studio-border);border-radius:8px;list-style:none}.send-options{position:absolute;z-index:20;top:calc(100% + 4px);left:0;min-width:170px;padding:5px;border:1px solid var(--studio-border);border-radius:9px;background:var(--studio-panel);box-shadow:0 8px 24px var(--studio-tone-0006)}.send-options button{display:block;width:100%;text-align:left;background:transparent;color:var(--studio-text);border-radius:5px;font-weight:400}.send-options button:hover{background:var(--studio-tone-1b2a40)}span[role=alert]{color:var(--studio-tone-ffb2be);max-width:270px}
</style>
