<script lang="ts">
  import {tick,onDestroy} from 'svelte';
  import {renameRecording,recordingTitleError} from './recording-titles.js';
  let {name,kind,id,disabled=false,maxLength=120,onSave}:{name:string;kind:string;id:string;disabled?:boolean;maxLength?:number;onSave?:(title:string)=>Promise<unknown>}=$props();
  let dialog=$state<HTMLDialogElement>(),button=$state<HTMLButtonElement>(),input=$state<HTMLInputElement>(),draft=$state(''),busy=$state(false),error=$state(''),alive=true;
  async function open(){if(disabled||busy)return;draft=name;error='';dialog?.showModal();await tick();input?.focus();input?.select();}
  function close(){if(!busy)dialog?.close();}
  async function save(){if(busy)return;error=recordingTitleError(draft,maxLength);if(error)return;busy=true;
    try{await (onSave?onSave(draft.trim()):renameRecording(kind,id,draft));if(alive){busy=false;dialog?.close();}}
    catch(cause){if(alive)error=typeof cause==='string'?cause:cause instanceof Error?cause.message:'The title was not saved. Please retry.';}
    finally{if(alive)busy=false;}
  }
  onDestroy(()=>{alive=false;dialog?.close();});
</script>
<button class="rename-recording" bind:this={button} {disabled} aria-label={`Rename ${name}`} title="Rename card" onclick={open}><svg viewBox="0 0 24 24" aria-hidden="true"><path d="m14 5 5 5M4 20l4-1 12-12a2 2 0 0 0-5-5L3 14l-1 6h6M13 20h9"/></svg></button>
<dialog bind:this={dialog} aria-label="Rename recording" oncancel={event=>{if(busy)event.preventDefault();}} onclose={()=>button?.focus({preventScroll:true})}>
  <form onsubmit={event=>{event.preventDefault();void save();}}><h2>Rename card</h2><p>Choose a title to find this recording in history.</p><label for={`rename-${kind}-${id}`}>Card title</label><input id={`rename-${kind}-${id}`} bind:this={input} bind:value={draft} disabled={busy} maxlength={maxLength*2} autocomplete="off"/><small>{Array.from(draft.trim()).length} / {maxLength}</small>{#if error}<p class="rename-error" role="alert">{error}</p>{/if}<div class="rename-actions"><button type="button" class="secondary" disabled={busy} onclick={close}>Cancel</button><button type="submit" disabled={busy||!!recordingTitleError(draft,maxLength)}>{busy?'Saving…':'Save title'}</button></div></form>
</dialog>
<style>
  .rename-recording{width:30px;height:30px;padding:6px;display:grid;place-items:center;flex:none;border:0;background:transparent;color:var(--studio-muted);border-radius:7px}.rename-recording:hover{color:var(--studio-text);background:var(--studio-tone-1b2a40)}.rename-recording svg{width:17px;height:17px;fill:none;stroke:currentColor;stroke-width:1.6;stroke-linecap:round;stroke-linejoin:round}dialog{width:min(430px,calc(100vw - 32px));max-height:calc(100vh - 32px);overflow:auto;box-sizing:border-box;padding:24px;background:var(--studio-panel);color:var(--studio-text);border:1px solid var(--studio-border);border-radius:16px;box-shadow:0 20px 80px #0006}dialog::backdrop{background:#0008}h2{font-size:18px;margin:0 0 10px}p{font-size:12px;line-height:1.7;color:var(--studio-muted);margin:0 0 18px}label{display:block;font-size:12px;margin:0 0 8px}input{width:100%;box-sizing:border-box;min-width:0;font:inherit;font-size:14px;padding:11px 12px;color:var(--studio-text);background:var(--studio-page);border:1px solid var(--studio-border);border-radius:8px}small{display:block;margin-top:8px;font-size:10px;color:var(--studio-muted)}.rename-actions{display:flex;justify-content:flex-end;gap:10px;margin-top:22px}.rename-actions button{font-size:12px;padding:9px 16px}.rename-error{color:var(--studio-tone-ffb2be);margin:12px 0 0}input:focus-visible,button:focus-visible{outline:2px solid var(--studio-focus);outline-offset:2px}
</style>
