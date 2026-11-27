<script lang="ts">
  import {onMount,tick} from 'svelte';
  import {invoke} from '@tauri-apps/api/core';
  import {shortcutLabel,shortcutFromKey,canSaveShortcut} from './shortcut-input.js';
  type Shortcut=import('./shortcut-input.js').Shortcut;
  type Status={supported:boolean;enabled:boolean;requested:boolean;inputMonitoring:boolean;accessibility:boolean;platform?:string;shortcut?:Shortcut;shortcutLabel?:string;defaultShortcut?:Shortcut};
  let status=$state<Status|null>(null),busy=$state(false),error=$state(''),editing=$state(false),draft=$state<Shortcut|null>(null),captureButton=$state<HTMLButtonElement>();
  let alive=true;
  const platform=$derived(status?.platform??'macos');
  const defaultShortcut=$derived(status?.defaultShortcut??(platform==='windows'?{modifiers:9,key:null}:{modifiers:0,key:'Fn'}));
  const binding=$derived(status?.shortcut??defaultShortcut);
  const label=$derived(status?.shortcutLabel??shortcutLabel(binding,platform));
  async function refresh(){try{const next=await invoke<Status>('get_fn_shortcut_status');if(alive)status=next;}catch{/* Browser preview. */}}
  async function change(enabled:boolean){busy=true;error='';try{const next=await invoke<Status>('set_fn_shortcut_enabled',{enabled});if(alive)status=next;}catch{if(alive)error='Could not change dictation. Finish any active recording, then try again.';}finally{busy=false;}}
  async function beginCapture(){
    error='';busy=true;
    try{await invoke('set_dictation_shortcut_capture',{capturing:true});if(!alive){void invoke('set_dictation_shortcut_capture',{capturing:false}).catch(()=>{});return;}editing=true;draft=null;await tick();captureButton?.focus();}
    catch{if(alive)error='Finish dictation before changing the shortcut.';}
    finally{busy=false;}
  }
  function cancelCapture(){editing=false;draft=null;void invoke('set_dictation_shortcut_capture',{capturing:false}).catch(()=>{});}
  function capture(event:KeyboardEvent){
    if(event.code==='Escape'){event.preventDefault();cancelCapture();return;}
    const next=shortcutFromKey(event);if(!next)return;event.preventDefault();event.stopPropagation();draft=next;error='';
  }
  async function save(shortcut:Shortcut){
    busy=true;error='';
    try{const next=await invoke<Status>('set_dictation_shortcut',{shortcut});if(alive){status=next;editing=false;draft=null;}}
    catch(cause){if(alive)error=typeof cause==='string'&&cause.length<350?cause:'Could not save the shortcut. Your previous shortcut is unchanged.';}
    finally{busy=false;}
  }
  onMount(()=>{void refresh();const timer=setInterval(()=>void refresh(),3000);return()=>{alive=false;clearInterval(timer);if(editing)cancelCapture();};});
</script>
<svelte:window onblur={()=>{if(editing)cancelCapture();}}/>
{#if status&&(status.supported||platform==='windows')}
  <div class="shortcut-row">
    <div><h3>Dictation shortcut</h3><p>Hold <strong>{label}</strong> to speak into your default microphone. Release to insert text into your app and save it in Studio.</p>
      <small>Studio needs to stay open. Some apps use the clipboard for insertion.</small>
      {#if status.supported&&status.requested&&(!status.inputMonitoring||!status.accessibility)}<p class="permissions" role="status">Allow Ethiopic Voice Studio in macOS Settings → Privacy &amp; Security: {#if !status.inputMonitoring}Input Monitoring{/if}{#if !status.inputMonitoring&&!status.accessibility} and {/if}{#if !status.accessibility}Accessibility{/if}. Then return here.</p>{/if}
      {#if platform==='macos'&&binding.key==='Fn'}<small>For a dedicated dictation key, set macOS Keyboard → “Press 🌐 key to” → “Do Nothing”. Fn + other keys keep their normal behavior.</small>{/if}
      {#if !status.supported}<small>Shortcut activation is not available in this build. Your Windows preference can still be saved.</small>{/if}
      {#if error}<p class="permissions" role="alert">{error}</p>{/if}
    </div>
    <div class="shortcut-control"><div class="shortcut-actions"><button class="secondary" disabled={busy||editing} onclick={beginCapture}>Change shortcut</button><button class="secondary" disabled={busy||editing} onclick={()=>save(defaultShortcut)}>Reset shortcut</button></div>
      <button class="secondary" disabled={busy||editing||!status.supported} onclick={()=>change(!status?.enabled)}>{busy?'Updating…':status.enabled?'Disable dictation shortcut':status.requested?'Check permissions':binding.key==='Fn'?'Enable Fn dictation':'Enable dictation shortcut'}</button><span role="status">{editing?'Paused while changing shortcut':status.enabled?`Ready · hold ${label}`:status.requested?'Waiting for permission':'Off'}</span>
    </div>
    {#if editing}<div class="shortcut-editor" role="group" aria-label="Change dictation shortcut"><button class="shortcut-capture secondary" bind:this={captureButton} onkeydown={capture} aria-label="Press new dictation shortcut">{draft?shortcutLabel(draft,platform):'Press your new shortcut'}</button><p>Hold two modifier keys, or a modifier with a letter, number, Space or F1–F12. Press Escape to cancel.</p><div class="shortcut-actions">{#if platform==='macos'}<button class="secondary" onclick={()=>draft={modifiers:0,key:'Fn'}}>Use Fn</button>{/if}<button disabled={busy||!canSaveShortcut(draft,platform)} onclick={()=>draft&&save(draft)}>Save shortcut</button><button class="secondary" disabled={busy} onclick={cancelCapture}>Cancel</button></div></div>{/if}
  </div>
{/if}
<style>
  .shortcut-row{display:grid;grid-template-columns:minmax(0,1fr) minmax(200px,320px);align-items:center;gap:28px;border-top:1px solid var(--studio-divider);border-bottom:1px solid var(--studio-divider);padding:25px 0}.shortcut-row h3{font-size:15px;margin:0 0 7px}.shortcut-row p{color:var(--studio-muted);font-size:13px;line-height:1.7;margin:0}.shortcut-row small{display:block;font-size:11px;color:var(--studio-muted);margin-top:8px;line-height:1.7}.shortcut-control{display:flex;flex-direction:column;align-items:flex-end;gap:8px}.shortcut-control button,.shortcut-editor button{font-size:12px}.shortcut-control span{font-size:11px;color:var(--studio-muted)}.shortcut-row p.permissions{margin-top:10px;color:var(--studio-gold)}.shortcut-actions{display:flex;flex-wrap:wrap;gap:8px}.shortcut-actions button{padding:8px 12px}.shortcut-editor{grid-column:1/-1;border:1px solid var(--studio-border);border-radius:12px;padding:18px}.shortcut-capture{width:100%;border-style:dashed;min-height:56px}.shortcut-editor p{font-size:11px;margin:10px 0 14px}@media(max-width:750px){.shortcut-row{grid-template-columns:1fr;gap:16px}.shortcut-control{align-items:flex-start}}
</style>
