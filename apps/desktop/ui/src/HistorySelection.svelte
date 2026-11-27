<script lang="ts">
  import RecordingDeleteDialog from './RecordingDeleteDialog.svelte';
  import {recordingSelectionKey,pruneRecordingSelection} from './recording-selection.js';
  import type {HistoryEntry,HistoryDeleteItem,HistoryDeleteResult} from './history-selection-types';
  let {entries,selected=$bindable<string[]>([]),busy=$bindable(false),disabled=false,onDelete,label='History selection',selectAllLabel='Select all recordings',deleteLabel='Delete selected',singular='recording',plural='recordings',description='This removes the selected app-owned audio copies, saved profiles, edit history and related metadata. Original files and exported copies stay unchanged.'}:{label?:string;selectAllLabel?:string;deleteLabel?:string;singular?:string;plural?:string;description?:string;entries:HistoryEntry[];selected?:string[];busy?:boolean;disabled?:boolean;onDelete:(items:HistoryDeleteItem[])=>Promise<HistoryDeleteResult>}=$props();
  let all=$state<HTMLInputElement>(),target=$state<{name:string}|null>(null),items=$state<HistoryDeleteItem[]>([]),names=$state<string[]>([]),error=$state(''),notice=$state('');
  const valid=$derived(pruneRecordingSelection(selected,entries));
  const locked=$derived(disabled||busy);
  $effect(()=>{if(valid.length!==selected.length||valid.some((key,i)=>key!==selected[i]))selected=valid;});
  $effect(()=>{if(all)all.indeterminate=valid.length>0&&valid.length<entries.length;});
  $effect(()=>{if(notice){const timer=setTimeout(()=>notice='',4000);return()=>clearTimeout(timer);}});
  function request(){if(locked||!valid.length)return;const wanted=new Set(valid);const chosen=entries.filter(entry=>wanted.has(recordingSelectionKey(entry.kind,entry.id)));items=chosen.map(entry=>({kind:entry.kind,id:entry.id,...(entry.revision!==undefined?{revision:entry.revision}:{})}));names=chosen.map(entry=>entry.name);target={name:names[0]};error='';}
  function close(){if(!busy){target=null;error='';}}
  async function confirm(){
    if(locked||!target)return;busy=true;error='';notice='';
    try{
      const snapshot=items.map(item=>({...item}));
      const result=await onDelete(snapshot),removed=new Set(result.deleted.map(item=>recordingSelectionKey(item.kind,item.id)));
      selected=selected.filter(key=>!removed.has(key));items=items.filter(item=>!removed.has(recordingSelectionKey(item.kind,item.id)));
      if(result.failed.length){names=entries.filter(entry=>selected.includes(recordingSelectionKey(entry.kind,entry.id))).map(entry=>entry.name);target={name:names[0]??'Remaining recordings'};error=`${result.deleted.length} deleted. ${result.failed.length} could not be deleted. The remaining recordings are still selected; retry or keep them.`;}
      else{target=null;names=[];notice=`${result.deleted.length} ${result.deleted.length===1?singular:plural} deleted from this app. Original files are unchanged.`;}
    }catch{error='Deletion did not finish. Finish active voice tasks, refresh history, and try again. Original files and exports are unchanged.';}
    finally{busy=false;}
  }
</script>
<div class="history-selection" role="group" aria-label={label}>
  <label><input bind:this={all} type="checkbox" aria-label={selectAllLabel} checked={entries.length>0&&valid.length===entries.length} disabled={locked||!entries.length} onchange={event=>selected=event.currentTarget.checked?entries.map(entry=>recordingSelectionKey(entry.kind,entry.id)):[]}/><span>{valid.length?`${valid.length} selected`:'Select all'}</span></label>
  {#if valid.length}<button class="secondary delete-selected" disabled={locked} onclick={request}>{deleteLabel}</button><button class="secondary" disabled={locked} onclick={()=>selected=[]}>Clear</button>{/if}
</div>
{#if notice}<p class="selection-notice" role="status">{notice}</p>{/if}
<RecordingDeleteDialog {target} {busy} blocked={disabled} {error} count={items.length} {names} {singular} {plural} {description} onCancel={close} onConfirm={confirm}/>
<style>
  .history-selection{display:flex;align-items:center;gap:8px;flex-wrap:wrap;padding:0 18px 14px;font-size:11px;color:var(--studio-muted);flex:none}.history-selection label{display:flex;align-items:center;gap:7px;margin:0}.history-selection input{width:15px;height:15px;margin:0;accent-color:var(--studio-accent)}button{font-size:10px;padding:6px 8px}.delete-selected{color:var(--studio-tone-ffb6bf);border-color:var(--studio-tone-754252)}.selection-notice{padding:0 18px 12px;margin:0;font-size:11px;line-height:1.7;color:var(--studio-tone-86ddbd)}input:focus-visible{outline:2px solid var(--studio-focus);outline-offset:3px}
</style>
