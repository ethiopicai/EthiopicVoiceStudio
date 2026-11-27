<script lang="ts">
 import {onMount} from 'svelte';
 import {invoke} from '@tauri-apps/api/core';
 import {reviewDownload,downloadSize} from './download-review.js';
 let {disabled=false,details=false}:{disabled?:boolean;details?:boolean}=$props();
 type Status={state:string;supported:boolean;ready:boolean;enabled:boolean;stage:string;received:number;total:number;error?:string|null;device?:string|null;liveReady:boolean};
 let status=$state<Status|null>(null),saving=$state(false),error=$state('');
 let alive=true,checking=false;
 const installing=$derived(status?.state==='installing');
 async function refresh(){if(checking)return;checking=true;try{const value=await invoke<Status>('get_noise_reduction_status');if(alive&&value&&typeof value.enabled==='boolean')status=value;}catch{/* Optional controls stay absent in unconnected previews. */}finally{checking=false;}}
 async function change(event:Event){const input=event.currentTarget as HTMLInputElement;saving=true;error='';try{const next=await invoke<Status>('set_noise_reduction_enabled',{enabled:input.checked});if(alive)status=next;}catch(cause){if(alive){input.checked=status?.enabled??false;error=typeof cause==='string'?cause:'Could not save noise reduction. Please retry.';}}finally{if(alive)saving=false;}}
 async function setup(){saving=true;error='';try{const approvedPlan=await reviewDownload('noise');if(!approvedPlan)return;const value=await invoke<Status>('start_noise_reduction_setup',{approvedPlan});if(alive)status=value;}catch(cause){if(alive)error=typeof cause==='string'?cause:'Noise reduction setup could not start. Try again.';}finally{if(alive)saving=false;}}
 async function pause(){try{await invoke('pause_noise_reduction_setup');}catch{if(alive)error='Could not pause setup. Please retry.';}}
 onMount(()=>{void refresh();const timer=setInterval(()=>void refresh(),1500);return()=>{alive=false;clearInterval(timer);};});
</script>
{#if status&&(status.supported||details)}
 <section class="noise-control" class:full={details} aria-label="Recognition noise reduction">
  {#if details}<h3>Speech recognition</h3>{/if}
  <div class="control-row"><label><input type="checkbox" checked={status.enabled} disabled={disabled||saving||installing||!status.ready} onchange={change}/> Reduce background noise</label>
   {#if !status.ready&&status.supported&&!installing}<button class="secondary" disabled={disabled||saving} onclick={setup}>{saving?'Preparing…':status.state==='failed'||status.state==='paused'?'Resume setup':`Set up · ${downloadSize(status.total)}`}</button>{/if}
  </div>
  {#if installing}<div class="download" role="status"><span>{status.stage}</span><progress aria-label="Noise reduction setup" max="1" value={status.received<status.total?status.received/status.total:undefined}></progress><button class="secondary" onclick={pause}>Pause setup</button></div>
  {:else if !status.supported}<p>{status.stage}</p>
  {:else if status.enabled&&!status.liveReady}<p>Available for recordings and Transcribe after stopping. Live noise reduction is too slow on this computer.</p>
  {:else if details}<p>Optional processing for Transcription and Dictation. Original recordings stay unchanged. May remove quieter overlapping voices.</p>{/if}
  {#if error||status.error}<p class="error" role="alert">{error||status.error}</p>{/if}
  {#if details&&status.ready}<details><summary>Noise reduction details</summary><dl><div><dt>Model</dt><dd>Inworld Voice Isolator v1 · realtime</dd></div><div><dt>Cache</dt><dd>Ready · private runtime overlay</dd></div><div><dt>Device</dt><dd>{status.device==='mps'?'Apple GPU':'CPU'}</dd></div><div><dt>Live processing</dt><dd>{status.liveReady?'Qualified locally':'Use transcription after stopping'}</dd></div><div><dt>License</dt><dd>Apache-2.0 · Inworld AI</dd></div></dl><p>The model stays ready with the shared recognition engine. Unload Transcription &amp; Dictation in Local models to free its memory.</p><button class="secondary" disabled={disabled||saving||installing} onclick={setup}>Repair noise reduction</button></details>{/if}
 </section>
{/if}
<style>
 .noise-control{width:100%;font-size:11px;color:var(--studio-muted)}.full{padding:20px 0;border-top:1px solid var(--studio-border)}h3{font-size:15px;color:var(--studio-text);margin:0 0 12px}.control-row{display:flex;flex-wrap:wrap;align-items:center;justify-content:center;gap:10px 18px}.full .control-row{justify-content:space-between}label{display:flex;align-items:center;gap:8px;cursor:pointer;color:var(--studio-text)}input{accent-color:var(--studio-accent)}p{line-height:1.7;margin:8px 0}.noise-control button{font-size:11px;padding:6px 10px}.download{display:flex;flex-wrap:wrap;align-items:center;gap:10px;margin-top:10px}.download progress{flex:1;min-width:100px;accent-color:var(--studio-accent)}.error{color:var(--studio-gold)}details{margin-top:12px}summary{cursor:pointer;color:var(--studio-accent)}dl div{display:flex;justify-content:space-between;gap:12px;padding:4px 0}dd{margin:0;text-align:right}
</style>
