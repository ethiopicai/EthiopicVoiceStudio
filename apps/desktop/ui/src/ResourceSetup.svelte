<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import {reviewDownload,downloadSize} from './download-review.js';
  let { onReady = () => {},installed=false,prominent=false }: {onReady?:()=>void;installed?:boolean;prominent?:boolean} = $props();
  type Progress={state:string;stage:string;downloadedBytes:number;totalBytes:number;canInstall:boolean;ready:boolean;requiresConsent:boolean;steps:{id:string;title:string;state:string}[];error?:string|null};
  let progress=$state<Progress|null>(null),error=$state(''),busy=$state(false),consent=$state(false);
  let alive=true,polling=false,lastState='';
  const desktop='__TAURI_INTERNALS__' in window;
  const active=$derived(!!progress&&['preparing','downloading','installing','checking','activating'].includes(progress.state));
  const percent=$derived(progress?.totalBytes?Math.min(100,Math.floor(progress.downloadedBytes/progress.totalBytes*100)):0);
  const size=downloadSize;
  function applyProgress(next:Progress){if(alive){progress=next;if(next.ready&&lastState!=='installed'&&lastState!=='completed'){onReady();window.dispatchEvent(new Event('studio-tools-changed'));}lastState=next.state;}}
  async function refresh(){
    if(polling||!desktop)return;polling=true;
    try{const next=await invoke<Progress>('get_voice_setup');applyProgress(next);}
    catch{if(alive)error='Setup status could not be checked. Please try again.';}
    finally{polling=false;}
  }
  async function control(){
    if(busy)return;busy=true;error='';
    try{if(active)await invoke('pause_voice_setup');else{const approvedPlan=await reviewDownload('voice-tools');if(!approvedPlan)return;await invoke('start_voice_setup',{approvedPlan,acceptedConsent:consent});}await refresh();}
    catch(cause){if(alive)error=String(cause);}
    finally{if(alive)busy=false;}
  }
  onMount(()=>{alive=true;void refresh();const changed=()=>void refresh();window.addEventListener('studio-tools-changed',changed);const update=(event:Event)=>applyProgress((event as CustomEvent<Progress>).detail);window.addEventListener('studio-setup-progress',update);const daily=setInterval(()=>void refresh(),24*60*60*1000);return()=>{alive=false;window.removeEventListener('studio-setup-progress',update);clearInterval(daily);window.removeEventListener('studio-tools-changed',changed);};});
</script>

{#if !progress?.ready||progress?.canInstall||active||progress?.error||error}
<section id="voice-tools-setup" class="card resource-setup" class:setup-prominent={prominent} tabindex="-1" aria-label="Voice tools setup">
  <div class="section-heading"><div><h2>{progress?.ready?'Your voice tools are ready':'Set up your voice tools'}</h2><p>Transcription, Dictation and Text to Speech.</p></div><span class="pill">On your computer</span></div>
  {#if !progress?.ready}<p>One simple setup. We download the right files, prepare the tools and make sure they work on your computer.</p>{/if}
  {#if !desktop}<p class="hint">Open the desktop app to prepare this computer.</p>
  {:else if !progress}<p role="status">Checking setup…</p>
  {:else}
    <div class="setup-status" role="status"><span class="setup-pulse" class:moving={active} aria-hidden="true"></span><span>{progress.stage}</span></div>
    <ol class="setup-steps" aria-label="Voice setup steps">{#each progress.steps??[] as step,index}<li class:done={step.state==='done'} class:current={step.state==='active'} class:attention={['paused','failed'].includes(step.state)} aria-current={step.state==='active'?'step':undefined}><span class="step-marker" aria-hidden="true">{step.state==='done'?'✓':index+1}</span><span>{step.title}<small>{step.state==='done'?'Done':step.state==='active'?'In progress…':step.state==='paused'?'Paused':step.state==='failed'?'Needs another try':'Waiting'}</small></span></li>{/each}</ol>
    {#if progress.canInstall&&!progress.ready}
      {#if active||progress.totalBytes>0&&['paused','failed'].includes(progress.state)}
      <div class="progress-meta"><span>{['preparing','installing','checking','activating'].includes(progress.state)?'Finishing setup':progress.state==='completed'?'Ready':progress.state==='installed'?'Installed':`${percent}% downloaded`}</span><span>{progress.totalBytes>0?`${size(progress.downloadedBytes)} / ${size(progress.totalBytes)}`:'This can take a few minutes.'}</span></div>
      {#if active&&progress.state!=='downloading'}<progress max="100" aria-label="Voice tools setup progress">Working…</progress>
      {:else}<progress max="100" value={percent} aria-label="Voice tools download progress">{percent}%</progress>{/if}
      {/if}
      <p class="hint">Downloads come from EthiopicAI’s public Hugging Face resources. Your recordings and transcripts stay on this computer.</p>
      {#if progress.requiresConsent&&!active}<label class="setup-consent"><input type="checkbox" bind:checked={consent}/>Studio is licensed for non-commercial use; model licenses are separate. I have permission to use my reference recordings.</label>{/if}
      <div class="setup-controls"><button onclick={control} disabled={busy||(!active&&progress.requiresConsent&&!consent)||progress.state==='activating'}>{busy?'Please wait…':active?(progress.state==='downloading'?'Pause download':'Pause setup'):progress.state==='paused'||progress.state==='failed'?'Continue setup':['completed','installed'].includes(progress.state)?'Repair voice tools':'Download voice tools'}</button><span>Pause or close the app and continue later. Setup keeps the previous working tools safe.</span></div>
    {/if}
    {#if progress.ready}<button class="secondary" onclick={control} disabled={busy}>Check voice tools again</button>{/if}
    {#if progress.error}<p class="notice error" role="alert">{progress.error}</p>{/if}
  {/if}
  {#if error}<p class="notice error" role="alert">{error}</p><button class="secondary" onclick={()=>{error='';void refresh();}}>Check again</button>{/if}
</section>
{/if}

<style>
  .setup-steps{display:grid;grid-template-columns:repeat(3,minmax(0,1fr));list-style:none;padding:0;margin:22px 0;gap:14px}.setup-steps li{display:flex;align-items:center;gap:10px;padding:15px 12px;border:1px solid var(--studio-border);border-radius:12px;font-size:12px;color:var(--studio-muted)}.step-marker{display:grid;place-items:center;flex:none;width:28px;height:28px;border-radius:50%;border:1px solid var(--studio-border);font-weight:700}.setup-steps small{display:block;margin-top:5px;font-size:10px}.setup-steps .done{color:var(--studio-accent)}.done .step-marker{background:var(--studio-tone-112b2c);border-color:var(--studio-accent)}.setup-steps .current{border-color:var(--studio-accent);color:var(--studio-text)}.setup-steps .attention{border-color:var(--studio-gold)}.setup-consent{display:flex;align-items:flex-start;gap:10px;font-size:12px;line-height:1.7;margin:18px 0;color:var(--studio-muted)}.setup-consent input{accent-color:var(--studio-accent);margin-top:4px;flex:none}@media(max-width:800px){.setup-steps{grid-template-columns:1fr;gap:8px}.setup-steps li{padding:12px}.setup-steps small{display:inline;margin-left:12px}}
  .resource-setup{scroll-margin-top:96px}.resource-setup.setup-prominent{border:2px solid var(--studio-accent);border-left:6px solid var(--studio-gold);background:linear-gradient(120deg,var(--studio-tone-112b2c),var(--studio-panel));box-shadow:0 8px 28px #0001}.resource-setup:focus-visible{outline:2px solid var(--studio-gold);outline-offset:4px}.setup-prominent .section-heading h2{font-size:24px}.setup-prominent .setup-controls button{font-size:14px;padding:13px 20px}
  .resource-setup>p{font-size:13px;color:var(--studio-tone-a9b7cf);line-height:1.7}.resource-setup .section-heading{justify-content:space-between;gap:16px}.setup-status{display:flex;align-items:center;gap:12px;min-height:64px;margin:14px 0;font-size:13px;line-height:1.6}.setup-pulse{flex:none;width:12px;height:12px;border-radius:50%;background:var(--studio-tone-2cd4a3);box-shadow:0 0 0 5px var(--studio-tone-2cd4a31a)}.setup-pulse.moving{animation:breathe 1.8s ease-in-out infinite}.progress-meta{display:flex;justify-content:space-between;gap:12px;font-size:12px;color:var(--studio-tone-acbad1);margin:12px 0}progress{width:100%;height:9px;accent-color:var(--studio-tone-2cd4a3)}.setup-controls{display:flex;align-items:center;gap:16px;flex-wrap:wrap;margin:20px 0 0}.setup-controls span{max-width:360px;color:var(--studio-tone-94a5c1);font-size:12px;line-height:1.6}@keyframes breathe{50%{opacity:.4;transform:scale(.8)}}@media(prefers-reduced-motion:reduce){.setup-pulse.moving{animation:none}}
</style>
