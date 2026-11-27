<script lang="ts">
  import { onMount, tick, type Snippet } from 'svelte';
  import type { Transcript } from './transcript-types';
  import {recordingTitles,displayRecordingTitle,refreshRecordingTitles} from './recording-titles.js';
  import RecordingDeleteDialog from './RecordingDeleteDialog.svelte';
  import {recordingSelectionKey,pruneRecordingSelection,selectedRecordingItems} from './recording-selection.js';
  import type {RecordingDeleteItem,RecordingBatchDeleteResult} from './recording-selection-types';
  type Kind='transcript'|'recording';
  type Entry={id:string;name:string;kind:Kind;revision?:number};
  let {history,drafts=[],selectedId='',historyOpen=$bindable(true),onSelect,onDelete,onRefresh,busy=false,selection=$bindable<string[]>([]),batchBusy=$bindable(false),onBatchDelete,children,input}: {
    history:Transcript[];drafts?:{id:string;name:string}[];selectedId?:string;historyOpen?:boolean;
    onSelect:(id:string,kind:Kind)=>void;onDelete:(id:string,kind:Kind)=>void;onRefresh:()=>void;busy?:boolean;selection?:string[];batchBusy?:boolean;onBatchDelete:(items:RecordingDeleteItem[])=>Promise<RecordingBatchDeleteResult>;children:Snippet;input:Snippet;
  }=$props();
  const entries=$derived<Entry[]>([...drafts.map(item=>({...item,name:displayRecordingTitle($recordingTitles,'recording',item.id,item.name),kind:'recording' as const})),...[...history].reverse().map(item=>({id:item.id,name:displayRecordingTitle($recordingTitles,'transcript',item.id,item.source_recording_id?displayRecordingTitle($recordingTitles,'recording',item.source_recording_id,item.name):item.name),revision:item.revision,kind:'transcript' as const}))]);
  const locked=$derived(busy||batchBusy);
  const selectedCount=$derived(pruneRecordingSelection(selection,entries).length);
  let selectAll=$state<HTMLInputElement>(),batchTarget=$state<{name:string}|null>(null),batchItems=$state<RecordingDeleteItem[]>([]),batchNames=$state<string[]>([]),batchError=$state(''),batchNotice=$state('');
  $effect(()=>{const pruned=pruneRecordingSelection(selection,entries);if(pruned.length!==selection.length||pruned.some((key,i)=>key!==selection[i]))selection=pruned;});
  $effect(()=>{if(selectAll)selectAll.indeterminate=selectedCount>0&&selectedCount<entries.length;});
  $effect(()=>{if(batchNotice){const timer=setTimeout(()=>batchNotice='',4000);return()=>clearTimeout(timer);}});
  function selectEverything(checked:boolean){selection=checked?entries.map(item=>recordingSelectionKey(item.kind,item.id)):[];}
  function requestBatch(){if(locked||!selectedCount)return;batchError='';batchItems=selectedRecordingItems(selection,entries);const wanted=new Set(selection);batchNames=entries.filter(item=>wanted.has(recordingSelectionKey(item.kind,item.id))).map(item=>item.name);batchTarget={name:batchNames[0]??'Selected recordings'};}
  function closeBatch(){if(!batchBusy){batchTarget=null;batchError='';}}
  async function confirmBatch(){
    if(!batchTarget||locked)return;batchBusy=true;batchError='';batchNotice='';
    try{
      const result=await onBatchDelete(batchItems);
      const removed=new Set(result.deleted.map(item=>recordingSelectionKey(item.kind,item.id)));
      for(const id of result.removedRecordingIds)removed.add(recordingSelectionKey('recording',id));
      selection=selection.filter(key=>!removed.has(key));
      if(result.failed.length){batchItems=batchItems.filter(item=>!removed.has(recordingSelectionKey(item.kind,item.id)));batchNames=entries.filter(item=>selection.includes(recordingSelectionKey(item.kind,item.id))).map(item=>item.name);batchTarget={name:batchNames[0]??'Remaining recordings'};batchError=`${result.deleted.length} deleted. ${result.failed.length} could not be deleted. The remaining recordings are still selected; try again or keep them.`;}
      else{batchTarget=null;batchNotice=`${result.deleted.length} recording${result.deleted.length===1?'':'s'} deleted from this app. Original files are unchanged.`;}
    }catch{batchError='Deletion could not be confirmed. Close active voice tasks, refresh history, and select the recordings again. Original files are unchanged.';}
    finally{batchBusy=false;}
  }
  let storageReady=$state(false);
  $effect(()=>{if(storageReady)try{localStorage.setItem('ethiopic.transcription.historyOpen',String(historyOpen));}catch{/* Collapse state remains usable in this session. */}});
  let layout=$state<HTMLElement>(),layoutWidth=$state(850),historyWidth=$state(260),resizing=$state(false);
  const maximum=$derived(Math.max(120,Math.min(640,layoutWidth-Math.min(240,layoutWidth*.35)-8)));
  const minimum=$derived(Math.min(180,maximum));
  const visibleWidth=$derived(Math.max(minimum,Math.min(maximum,historyWidth)));
  function setWidth(value:number){historyWidth=Math.round(Math.max(minimum,Math.min(maximum,value)));}
  function saveWidth(){try{localStorage.setItem('ethiopic.transcription.historyWidth',String(historyWidth));}catch{/* Resizing works without storage. */}}
  function beginResize(event:PointerEvent){if(event.button!==0)return;event.preventDefault();resizing=true;(event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);}
  function moveResize(event:PointerEvent){if(resizing&&layout)setWidth(event.clientX-layout.getBoundingClientRect().left);}
  function endResize(){if(resizing){resizing=false;saveWidth();}}
  function resizeKey(event:KeyboardEvent){const step=event.shiftKey?50:20;if(event.key==='ArrowLeft')setWidth(visibleWidth-step);else if(event.key==='ArrowRight')setWidth(visibleWidth+step);else if(event.key==='Home')setWidth(minimum);else if(event.key==='End')setWidth(maximum);else return;event.preventDefault();saveWidth();}
  let menu=$state<{entry:Entry;x:number;y:number}|null>(null),menuElement=$state<HTMLDivElement>();
  let menuTrigger:HTMLElement|null=null;
  $effect(()=>{if(menu)void tick().then(()=>menuElement?.querySelector<HTMLButtonElement>('button')?.focus());});
  function closeMenu(restore=false){menu=null;if(restore&&menuTrigger?.isConnected)menuTrigger.focus();}
  function openMenu(event:MouseEvent,entry:Entry){event.preventDefault();if(locked)return;menuTrigger=event.currentTarget as HTMLElement;menu={entry,x:Math.max(8,Math.min(event.clientX,window.innerWidth-198)),y:Math.max(8,Math.min(event.clientY,window.innerHeight-64))};}
  function menuKey(event:KeyboardEvent,entry:Entry){if(event.key==='ContextMenu'||event.shiftKey&&event.key==='F10'){event.preventDefault();if(locked)return;menuTrigger=event.currentTarget as HTMLElement;const bounds=menuTrigger.getBoundingClientRect();menu={entry,x:Math.max(8,Math.min(bounds.left,window.innerWidth-198)),y:Math.max(8,Math.min(bounds.bottom,window.innerHeight-64))};}}
  function deleteFromMenu(){if(!menu||locked)return;const entry=menu.entry;closeMenu(true);onDelete(entry.id,entry.kind);}
  function outsidePointer(event:PointerEvent){if(menu&&event.target instanceof Node&&!menuElement?.contains(event.target))closeMenu();}
  function windowKey(event:KeyboardEvent){if(menu&&event.key==='Escape'){event.preventDefault();event.stopPropagation();closeMenu(true);}}
  onMount(()=>{
    try{const collapsed=localStorage.getItem('ethiopic.transcription.historyOpen');if(collapsed!==null)historyOpen=collapsed==='true';const saved=Number(localStorage.getItem('ethiopic.transcription.historyWidth'));if(Number.isFinite(saved)&&saved>=120&&saved<=1000)historyWidth=saved;}catch{/* Keep default dimensions. */}
    storageReady=true;
    if(!layout)return;
    layoutWidth=layout.getBoundingClientRect().width;
    const observer=new ResizeObserver(entries=>{layoutWidth=entries[0].contentRect.width;});observer.observe(layout);
    return()=>observer.disconnect();
  });
</script>

<svelte:window onpointerdown={outsidePointer} onkeydown={windowKey}/>
<section class="recording-workspace" bind:this={layout} class:history-hidden={!historyOpen} class:resizing style={`--history-width:${visibleWidth}px`} aria-label="Recording workspace">
  {#if historyOpen}
    <section id="transcription-history" class="recording-history" aria-label="Transcription history">
      <div class="history-heading"><h2>History</h2><button class="secondary icon-button" onclick={()=>{closeMenu();historyOpen=false;}} aria-label="Hide transcription history" aria-expanded={true} aria-controls="transcription-history"><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 4h16v16H4V4Zm11 0v16m-4-12-4 4 4 4"/></svg></button></div>
      {#if entries.length}<div class="history-list">{#each entries as entry (`${entry.kind}-${entry.id}`)}<button class="history-item secondary" class:selected={selectedId===entry.id} disabled={locked} aria-current={selectedId===entry.id?'true':undefined} aria-haspopup="menu" title="Right-click for options" onclick={()=>{closeMenu();onSelect(entry.id,entry.kind);}} oncontextmenu={event=>openMenu(event,entry)} onkeydown={event=>menuKey(event,entry)}><strong>{entry.name}</strong><small>{entry.kind==='recording'?'Ready to transcribe':`Revision ${entry.revision}`}</small></button>{/each}</div>{:else}<div class="history-empty"><p>No transcriptions yet</p></div>{/if}
    </section>
    <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions (Focusable window separator supports the WAI-ARIA keyboard resize pattern.) -->
    <div class="history-resizer" role="separator" tabindex="0" aria-label="Resize transcription history" aria-orientation="vertical" aria-controls="transcription-history" aria-valuemin={Math.round(minimum)} aria-valuemax={Math.round(maximum)} aria-valuenow={Math.round(visibleWidth)} title="Drag to resize history. Use arrow keys when focused." onpointerdown={beginResize} onpointermove={moveResize} onpointerup={endResize} onpointercancel={endResize} onlostpointercapture={endResize} onkeydown={resizeKey} ondblclick={()=>{setWidth(260);saveWidth();}}></div>
  {/if}
  <section class="recording-main task-panel" aria-label="Audio and transcripts">
    <div class="recording-toolbar transcript-list-heading">{#if !historyOpen}<button class="secondary show-history" onclick={()=>historyOpen=true} aria-label="Show history" aria-expanded={false} aria-controls="transcription-history">Show history</button>{/if}<label class="batch-select-all"><input bind:this={selectAll} type="checkbox" aria-label="Select all recordings" checked={entries.length>0&&selectedCount===entries.length} disabled={locked||!entries.length} onchange={event=>selectEverything(event.currentTarget.checked)}/><span>{selectedCount?`${selectedCount} selected`:'Select all'}</span></label><span class="transcript-total">Transcripts · {history.length+drafts.length}</span>{#if selectedCount}<button class="secondary batch-delete" disabled={locked} onclick={requestBatch}>Delete selected</button><button class="secondary clear-selection" disabled={locked} onclick={()=>selection=[]}>Clear</button>{/if}<button class="secondary refresh-history" disabled={locked} onclick={()=>{void refreshRecordingTitles();onRefresh();}}>Refresh transcripts</button></div>
    {#if batchNotice}<p class="batch-notice" role="status">{batchNotice}</p>{/if}
    <!-- svelte-ignore a11y_no_noninteractive_tabindex (Keyboard focus allows scrolling the shared conversation.) -->
    <div class="recording-conversation transcript-card-list" data-card-scroller role="region" aria-label="Scrollable transcript cards" tabindex="0">{@render children()}</div>
    <div class="recording-input" data-recording-input>{@render input()}</div>
  </section>
</section>
{#if menu}<div class="history-context-menu" bind:this={menuElement} role="menu" aria-label="Transcription actions" style={`left:${menu.x}px;top:${menu.y}px`}><button role="menuitem" disabled={locked} onclick={deleteFromMenu} onkeydown={event=>{if(event.key==='ArrowDown'||event.key==='ArrowUp'||event.key==='Home'||event.key==='End'){event.preventDefault();(event.currentTarget as HTMLButtonElement).focus();}else if(event.key==='Tab'){closeMenu(true);}}}>Delete transcription</button></div>{/if}

<RecordingDeleteDialog target={batchTarget} busy={batchBusy} error={batchError} count={batchItems.length} names={batchNames} onCancel={closeBatch} onConfirm={confirmBatch}/>
<style>
  .recording-toolbar{flex-wrap:wrap}.batch-select-all{display:flex;align-items:center;gap:7px;margin:0;font-size:11px;white-space:nowrap;color:var(--studio-tone-b9c9de)}.batch-select-all input{width:15px;height:15px;margin:0;accent-color:var(--studio-accent)}.recording-toolbar .batch-delete{color:var(--studio-tone-ffb6bf);border-color:var(--studio-tone-754252)}.batch-notice{margin:0;padding:8px 28px;font-size:12px;color:var(--studio-tone-86ddbd);line-height:1.7}.batch-select-all input:focus-visible{outline:2px solid var(--studio-gold);outline-offset:3px}

  .recording-workspace{display:grid;grid-template-columns:var(--history-width) 8px minmax(0,1fr);width:100%;height:100%;min-height:0;min-width:0;overflow:hidden;color:var(--studio-tone-e9eefb);background:var(--studio-tone-080f1e)}.recording-workspace.history-hidden{grid-template-columns:minmax(0,1fr)}.recording-history{display:flex;flex-direction:column;min-height:0;min-width:0;background:var(--studio-tone-0b1322);overflow:hidden}.history-heading{display:flex;align-items:center;justify-content:space-between;gap:12px;min-height:66px;padding:12px 18px;box-sizing:border-box;flex-shrink:0}.history-heading h2{font-size:16px;margin:0;color:var(--studio-tone-e9eefb)}.icon-button{width:30px;height:30px;padding:5px;display:grid;place-items:center;flex-shrink:0;border:1px solid var(--studio-tone-34435b);border-radius:8px;background:transparent}.icon-button svg{width:18px;height:18px;fill:none;stroke:currentColor;stroke-width:1.6}.history-list{display:flex;flex-direction:column;gap:6px;overflow-y:auto;min-height:0;padding:0 10px 16px;scrollbar-gutter:stable}.history-item{display:flex;flex-direction:column;align-items:flex-start;gap:6px;width:100%;min-width:0;box-sizing:border-box;text-align:left;padding:12px;border:1px solid transparent;border-radius:9px;background:transparent;color:var(--studio-tone-cbd6e9);font:inherit}.history-item strong{font-size:12px;font-weight:500;max-width:100%;overflow-wrap:anywhere}.history-item small{font-size:10px;color:var(--studio-tone-91a2bd)}.history-item:hover,.history-item.selected{background:var(--studio-tone-142337);border-color:var(--studio-tone-2b4d50)}.history-item.selected{color:var(--studio-tone-e4fff3)}.history-item:disabled{opacity:.55;cursor:default}.history-empty{display:grid;place-items:center;flex:1;padding:20px;font-size:12px;text-align:center;color:var(--studio-tone-8e9fbc)}.history-resizer{position:relative;width:8px;min-width:8px;cursor:col-resize;touch-action:none;background:var(--studio-sidebar);border-left:1px solid var(--studio-divider);box-sizing:border-box;outline-offset:-2px}.history-resizer::after{content:'';position:absolute;left:2px;top:0;bottom:0;width:2px;background:transparent;border-radius:2px}.history-resizer:hover::after,.history-resizer:focus-visible::after,.resizing .history-resizer::after{background:var(--studio-accent)}.resizing{cursor:col-resize;user-select:none}.recording-main{display:flex;flex-direction:column;min-height:0;min-width:0;overflow:hidden}.recording-toolbar{display:flex;align-items:center;gap:12px;flex-shrink:0;min-height:52px;padding:12px 28px;box-sizing:border-box;font-size:11px;color:var(--studio-tone-96a7c2)}.recording-toolbar .refresh-history{margin-left:auto}.recording-toolbar button{font-size:10px;padding:6px 10px}.recording-conversation{display:flex;flex-direction:column;gap:16px;flex:1;min-height:0;min-width:0;overflow-y:auto;overflow-x:hidden;padding:10px 28px 22px;scrollbar-gutter:stable}.recording-input{display:block;flex:none;min-width:0;width:100%;margin:0;padding:18px 28px 16px;box-sizing:border-box;border-top:1px solid var(--studio-tone-202e42);background:var(--studio-tone-080f1e)}.history-context-menu{position:fixed;z-index:100;width:190px;padding:5px;background:var(--studio-tone-111b2d);border:1px solid var(--studio-tone-35455e);border-radius:10px;box-shadow:0 12px 30px var(--studio-tone-0008)}.history-context-menu button{width:100%;padding:10px 12px;text-align:left;border-radius:6px;border:0;background:transparent;color:var(--studio-tone-ffb6bf);font:inherit;font-size:12px;font-weight:500}.history-context-menu button:hover{background:var(--studio-tone-352333)}button:focus-visible,.history-resizer:focus-visible,.recording-conversation:focus-visible{outline:2px solid var(--studio-gold);outline-offset:2px}.history-resizer:focus-visible{outline-offset:-2px}.recording-conversation:focus-visible{outline-offset:-3px}@media(max-width:650px){.recording-toolbar{padding:10px 15px;gap:8px}.recording-conversation{padding:10px 15px 18px;gap:14px}.recording-input{padding:15px}}
</style>
