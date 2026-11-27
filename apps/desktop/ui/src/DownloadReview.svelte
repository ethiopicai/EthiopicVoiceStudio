<script lang="ts">
 import {onMount,tick} from 'svelte';
 import {invoke} from '@tauri-apps/api/core';
 import {downloadSize} from './download-review.js';
 type Plan={id:string;kind:string;title:string;available:boolean;reason:string;downloadBytes:number;items:{name:string;purpose:string;bytes:number;url:string}[]};
 let dialog=$state<HTMLDialogElement>(),plan=$state<Plan|null>(null),loading=$state(false),error=$state('');
 let pending:((id:string|null)=>void)|null=null,sequence=0;
 const hosts=$derived(plan?[...new Set(plan.items.map(item=>new URL(item.url).hostname))].join(', '):'');
 function finish(id:string|null){sequence++;const resolve=pending;pending=null;dialog?.close();plan=null;loading=false;resolve?.(id);}
 onMount(()=>{
  const receive=async(event:Event)=>{
   const detail=(event as CustomEvent).detail as {kind:string;resolve:(id:string|null)=>void};
   if(pending){detail.resolve(null);return;}
   pending=detail.resolve;const token=++sequence;plan=null;error='';loading=true;await tick();dialog?.showModal();
   try{
    const next=await invoke<Plan>('get_asset_download_plan',{kind:detail.kind});
    if(!next||!Array.isArray(next.items)||typeof next.available!=='boolean'||!Number.isSafeInteger(next.downloadBytes)||next.downloadBytes<0||!next.id||next.kind!==detail.kind||next.available&&!next.items.length&&next.kind!=='voice-tools'||next.items.some(item=>!Number.isSafeInteger(item.bytes)||item.bytes<=0||new URL(item.url).protocol!=='https:')||next.items.reduce((total,item)=>total+item.bytes,0)!==next.downloadBytes)throw Error('incomplete quote');
    if(token===sequence)plan=next;
   }catch{if(token===sequence)error='Download details could not be checked. No download was started. Close this window and try again.';}
   finally{if(token===sequence)loading=false;}
  };
  window.addEventListener('studio-download-review',receive);
  return()=>{window.removeEventListener('studio-download-review',receive);finish(null);};
 });
</script>
<dialog bind:this={dialog} aria-labelledby="download-review-title" oncancel={event=>{event.preventDefault();finish(null);}}>
 <h2 id="download-review-title">Review your download</h2>
 {#if loading}<p role="status">Checking the resources selected for this computer…</p>
 {:else if error}<p role="alert">{error}</p>
 {:else if plan}
  <h3>{plan.title}</h3>
  <p>These resources let this tool run on your computer. Studio installs everything for you.</p>
  {#if plan.kind==='voice-tools'}<p>We download your voice tools, prepare them and try a short practice run offline. Studio takes care of everything.</p>{/if}
  {#if plan.items.length}
   <dl class="download-total"><dt>Maximum download</dt><dd>{downloadSize(plan.downloadBytes)}</dd></dl>
   <ul>{#each [...new Set(plan.items.map(item=>item.purpose))] as purpose}<li><span>{purpose}</span><strong>{downloadSize(plan.items.filter(item=>item.purpose===purpose).reduce((sum,item)=>sum+item.bytes,0))}</strong></li>{/each}</ul>
   <p class="hint">Already verified files are reused, so the actual download may be smaller. Setup also needs temporary disk space.</p>
   <details><summary>All files and sizes ({plan.items.length})</summary><ul>{#each plan.items as item}<li><span>{item.name}</span><strong>{downloadSize(item.bytes)}</strong></li>{/each}</ul><p>Download sources: {hosts}</p></details>
  {/if}
  {#if plan.available&&!plan.items.length}<p>Your files are already on this computer. We will check that all the tools work together; no download is needed.</p>{/if}
  {#if !plan.available}<p class="unavailable" role="status">{plan.reason}</p>{/if}
  <p>Downloads use your internet connection. You can pause and continue later. After setup, this tool works offline. Your recordings and transcripts are not uploaded.</p>
 {/if}
 <div class="actions"><button class="secondary" onclick={()=>finish(null)}>Not now</button><button disabled={loading||!!error||!plan?.available} onclick={()=>plan&&finish(plan.id)}>{plan?.available&&plan.kind==='voice-tools'&&!plan.items.length?'Check voice tools':'Download and set up'}</button></div>
</dialog>
<style>
 dialog{width:min(540px,calc(100vw - 40px));max-height:85vh;overflow:auto;box-sizing:border-box;background:var(--studio-panel);color:var(--studio-text);border:1px solid var(--studio-border);border-radius:18px;padding:26px;box-shadow:0 24px 80px #0008}dialog::backdrop{background:#0009}h2{font-size:21px;margin:0 0 18px}h3{font-size:15px;margin:0 0 10px}p{font-size:12px;line-height:1.7;color:var(--studio-muted)}.download-total{display:flex;justify-content:space-between;gap:20px;padding:16px 0;font-size:16px;font-weight:600}.download-total dd{margin:0;color:var(--studio-accent)}ul{padding:0;margin:12px 0;list-style:none}li{display:flex;justify-content:space-between;gap:20px;border-bottom:1px solid var(--studio-border);padding:10px 0;font-size:12px}li span{min-width:0;overflow-wrap:anywhere}li strong{flex:none;font-size:12px}details{font-size:12px;margin:18px 0}details ul{max-height:200px;overflow:auto}summary{cursor:pointer;color:var(--studio-accent)}.hint{font-size:11px}.actions{display:flex;justify-content:flex-end;flex-wrap:wrap;gap:12px;margin-top:24px}.actions button{font-size:12px}.unavailable{color:var(--studio-gold)}
</style>
