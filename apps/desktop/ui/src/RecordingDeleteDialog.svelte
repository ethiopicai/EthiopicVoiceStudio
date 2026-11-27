<script lang="ts">
  import { onMount, tick } from 'svelte';
  let {target,busy,blocked=false,error='',onCancel,onConfirm,count=1,names=[],singular='transcription',plural='recordings',description='This removes the app’s saved transcript, corrections, recording copies, and related metadata. Your original files and exported copies stay unchanged.'}: {target:{name:string}|null;busy:boolean;blocked?:boolean;error?:string;onCancel:()=>void;onConfirm:()=>void;count?:number;names?:string[];singular?:string;plural?:string;description?:string}=$props();
  const dialogId=$props.id();
  let dialog=$state<HTMLDialogElement>(),keep=$state<HTMLButtonElement>();
  let previousFocus:HTMLElement|null=null;
  function focusable(element:HTMLElement){return element.isConnected&&!element.hasAttribute('disabled')&&element.tabIndex>=0&&element.getClientRects().length>0&&!element.closest('details:not([open]),dialog:not([open])')&&getComputedStyle(element).visibility!=='hidden';}
  function restoreFocus(){
    if(previousFocus&&focusable(previousFocus))previousFocus.focus();
    else Array.from(document.querySelectorAll<HTMLElement>('[data-recording-input] button:not([disabled]), [data-recording-input] input:not([disabled]), .record-button:not([disabled]), .transcription-import input:not([disabled]), .recording-toolbar button:not([disabled]), .edit-import-tools button:not([disabled]), .cleanup-drop button:not([disabled]), .voice-style select:not([disabled]), .history-selection input:not([disabled])')).find(focusable)?.focus();
    previousFocus=null;
  }

  $effect(()=>{
    if(!dialog)return;
    if(target&&!dialog.open){
      previousFocus=document.activeElement instanceof HTMLElement?document.activeElement:null;
      dialog.showModal();
      void tick().then(()=>{if(dialog?.open)keep?.focus();});
    }else if(!target&&dialog.open){dialog.close();restoreFocus();}
  });
  function cancel(){if(!busy)onCancel();}
  onMount(()=>()=>{if(dialog?.open){dialog.close();restoreFocus();}});
</script>

<dialog bind:this={dialog} class="recording-delete-dialog" aria-labelledby={`${dialogId}-title`} aria-describedby={`${dialogId}-details`} aria-busy={busy} oncancel={event=>{event.preventDefault();cancel();}}>
  {#if target}
    <h2 id={`${dialogId}-title`}>{count>1?`Delete ${count} ${plural}?`:`Delete ${singular}?`}</h2>
    {#if count>1}<p class="delete-selection-count">{count} {plural} selected</p>{#if names.length}<ul class="delete-names">{#each names.slice(0,5) as name}<li>{name}</li>{/each}</ul>{/if}{#if names.length&&count>Math.min(5,names.length)}<p class="delete-remaining">And {count-Math.min(5,names.length)} more.</p>{/if}{:else}<p class="delete-filename">{target.name}</p>{/if}
    <p id={`${dialogId}-details`}>{description}</p>
    {#if blocked&&!busy}<p role="status">Finish or cancel the active voice task before deleting this recording.</p>{/if}
    {#if error}<p class="delete-error" role="alert">{error}</p>{/if}
    <div class="delete-actions"><button bind:this={keep} class="secondary" disabled={busy} onclick={cancel}>Keep it</button><button class="delete-confirm" disabled={busy||blocked} onclick={onConfirm}>{busy?'Deleting…':'Delete from app'}</button></div>
  {/if}
</dialog>

<style>
  .delete-names{margin:8px 0 12px;padding-left:20px;color:var(--studio-tone-d7e1f1);font-size:13px;line-height:1.7;overflow-wrap:anywhere}.delete-selection-count,.delete-remaining{margin:8px 0}.delete-names li{padding:2px 0}

  .recording-delete-dialog{color:var(--studio-tone-e9eefb);background:var(--studio-tone-111b2d);border:1px solid var(--studio-tone-35455e);border-radius:15px;padding:24px;width:min(460px,calc(100vw - 40px));box-sizing:border-box;max-height:90vh;overflow-y:auto;box-shadow:0 20px 80px var(--studio-tone-0008)}.recording-delete-dialog::backdrop{background:var(--studio-tone-020611ba)}.recording-delete-dialog h2{font-size:19px;margin:0 0 15px;font-weight:600}.recording-delete-dialog p{font-size:13px;line-height:1.7;color:var(--studio-tone-b8c7de)}.recording-delete-dialog .delete-filename{font-weight:600;color:var(--studio-tone-e9eefb);overflow-wrap:anywhere}.recording-delete-dialog .delete-error{color:var(--studio-tone-ffc3cc);background:var(--studio-tone-301d27);border:1px solid var(--studio-tone-5a3443);padding:10px 12px;border-radius:8px}.delete-actions{display:flex;justify-content:flex-end;gap:10px;margin-top:22px}.delete-actions button{font-size:12px;padding:10px 14px;border-radius:8px}.delete-confirm{background:var(--studio-tone-a4344b);color:white;border:1px solid var(--studio-tone-c05268)}.delete-confirm:hover{background:var(--studio-tone-b23b54)}.delete-actions button:disabled{opacity:.55;cursor:default}.delete-actions button:focus-visible{outline:2px solid var(--studio-gold);outline-offset:3px}@media(max-width:450px){.recording-delete-dialog{padding:20px}.delete-actions{flex-wrap:wrap}}
</style>
