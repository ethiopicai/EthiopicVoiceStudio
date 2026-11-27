<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import fallbackHtml from './home-fallback.html?raw';
  import { HOME_FRAME_SANDBOX, HOME_FRAME_ALLOW, bundledHomeDocument,themedHomeDocument } from './home-frame.js';
  import themePalette from './theme.css?raw';
  import {effectiveTheme} from './preferences.js';
  import VoiceSetupNotice from './VoiceSetupNotice.svelte';
  import UpdateNotice from './UpdateNotice.svelte';
  let { onNavigate,setupNotice=null,onSetup=()=>onNavigate('setup'),onUpdates=()=>onNavigate('setup') }: {onNavigate:(target:import('./home-content.js').HomeTarget)=>void;setupNotice?:ReturnType<typeof import('./voice-setup.js').voiceSetupNotice>|null;onSetup?:()=>void;onUpdates?:()=>void}=$props();
  type HomePage = {html:string;source:'bundled'|'cache'|'remote';updatedAt?:number;configured:boolean;refreshDue?:boolean;notice?:string};
  let page=$state<HomePage>({html:bundledHomeDocument(fallbackHtml),source:'bundled',configured:false});
  let refreshing=$state(false);
  let message=$state('');
  let current=true;
  const desktop='__TAURI_INTERNALS__' in window;
  async function refresh() {
    if(!desktop || !page.configured || refreshing)return;
    refreshing=true;message='';
    try { const next=await invoke<HomePage>('refresh_home_page');if(current){page=next;message=next.notice?'Could not update right now. Your saved studio content is still available.':'';} }
    catch {if(current)message='Could not update right now. Your saved studio content is still available.';}
    finally {if(current)refreshing=false;}
  }
  onMount(()=>{
    current=true;
    if(desktop)void invoke<HomePage>('get_home_page').then(next=>{if(current){page=next;if(next.configured&&next.refreshDue)void refresh();}}).catch(()=>{if(current)message='Showing local studio content. You can continue working offline.';});
    return()=>{current=false;};
  });
</script>

<div class="home-page task-panel">
  {#if setupNotice?.visible}<VoiceSetupNotice notice={setupNotice} onOpen={onSetup}/>{/if}
  <UpdateNotice home onSettings={onUpdates}/>
  <section class="home-welcome"><div><span class="home-eyebrow">ሰላም · WELCOME</span><h1>Ethiopic Voice Studio</h1><p>Transcribe recordings and dictate text on your computer.</p><button onclick={()=>onNavigate('transcription')}>Start a transcript <span aria-hidden="true">↗</span></button></div><div class="home-illustration" aria-hidden="true"><div class="voice-orbit"><span></span><span></span><span></span><span></span><span></span><span></span><span></span></div><span class="orbit-label">Built for Amharic</span></div></section>
  <div class="home-quick"><span>Pick up a tool</span><button class="secondary" onclick={()=>onNavigate('transcription')}>Transcription</button><button class="secondary" onclick={()=>onNavigate('tts')}>Text to Speech</button><button class="secondary" onclick={()=>onNavigate('projects')}>Projects</button></div>
  <div class="home-content-heading"><h2>Around your studio</h2><div class="home-update-actions"><span class="home-preview">{page.source==='cache'?'Cached':page.source==='remote'?'Updated':'Preview'}</span><button class="secondary home-refresh" onclick={refresh} disabled={!desktop || !page.configured || refreshing} aria-label="Refresh studio tips and updates">{refreshing?'Updating…':'Refresh'}</button></div></div>
  {#if message}<p class="home-status" role="status">{message}</p>{/if}
  <iframe class="home-content-frame" title="Studio tips and updates" sandbox={HOME_FRAME_SANDBOX} allow={HOME_FRAME_ALLOW} referrerpolicy="no-referrer" srcdoc={themedHomeDocument(page.html,$effectiveTheme,themePalette)}></iframe>
  <p class="home-footnote">Tips and updates will appear here. Your audio and transcripts stay on this computer.</p>
</div>

<style>
  .home-content-frame{display:block;width:100%;height:600px;border:0;border-radius:14px;background:var(--studio-page);color-scheme:inherit}.home-update-actions{display:flex;align-items:center;gap:9px}.home-refresh{font-size:10px;padding:6px 11px}.home-status{font-size:11px;color:var(--studio-muted);margin:0 0 12px;line-height:1.7}
  .home-page{overflow-y:auto;padding:30px 34px 24px;flex:1;min-height:0}.home-welcome{display:flex;justify-content:space-between;gap:25px;align-items:center;background:linear-gradient(120deg,var(--studio-tone-112b2c),var(--studio-tone-101b30) 75%);border:1px solid var(--studio-tone-294744);border-radius:20px;padding:32px}.home-eyebrow{font-size:10px;letter-spacing:1.8px;color:var(--studio-tone-83e2b7);font-weight:600}.home-welcome h1{font-size:clamp(30px,4vw,45px);line-height:1.2;letter-spacing:-1.2px;margin:15px 0}.home-welcome p{color:var(--studio-tone-b2bfd6);font-size:13px;max-width:340px;line-height:1.8;margin:0 0 22px}.home-welcome button{font-size:12px;display:flex;align-items:center;gap:25px}.home-illustration{width:190px;flex-shrink:0;text-align:center}.voice-orbit{width:170px;height:170px;display:flex;align-items:center;justify-content:center;gap:7px;border-radius:50%;border:1px solid var(--studio-tone-2b5146);background:var(--studio-tone-122d30);margin:0 auto}.voice-orbit span{width:7px;height:28px;border-radius:6px;background:var(--studio-accent)}.voice-orbit span:nth-child(2),.voice-orbit span:nth-child(6){height:55px}.voice-orbit span:nth-child(3),.voice-orbit span:nth-child(5){height:85px;background:var(--studio-tone-83dca7)}.voice-orbit span:nth-child(4){height:110px;background:var(--studio-gold)}.orbit-label{display:block;margin-top:12px;font-size:10px;letter-spacing:1px;color:var(--studio-muted)}.home-quick{display:flex;flex-wrap:wrap;align-items:center;gap:9px;margin:24px 0 30px}.home-quick>span{font-size:11px;color:var(--studio-muted);margin-right:6px}.home-quick button{font-size:11px;font-weight:500;padding:8px 13px}.home-content-heading{display:flex;justify-content:space-between;align-items:center;gap:12px;margin-bottom:18px}.home-content-heading h2{font-size:17px;margin:0}.home-preview{font-size:9px;border:1px solid var(--studio-border);padding:4px 8px;border-radius:12px;color:var(--studio-muted);white-space:nowrap}.home-footnote{font-size:10px;line-height:1.8;color:var(--studio-tone-8595b2);margin:24px 0 0}@media(max-width:1000px){.home-page{padding:24px}.home-welcome{padding:26px}.home-illustration{width:130px}.voice-orbit{width:120px;height:120px;gap:5px}.voice-orbit span{width:5px}.voice-orbit span:nth-child(4){height:80px}.voice-orbit span:nth-child(3),.voice-orbit span:nth-child(5){height:60px}}@media(max-width:800px){.home-illustration{display:none}.home-page{padding:20px}.home-welcome{padding:24px}}
</style>
