<script lang="ts">
  import {onMount} from 'svelte';
  import {preferences,savePreferences,microphoneConstraints} from './preferences.js';
  import {dictationStartupMessage} from './dictation-startup.js';
  import FnShortcutSettings from './FnShortcutSettings.svelte';
  import BugReport from './BugReport.svelte';
  import NoiseReductionControl from './NoiseReductionControl.svelte';
  let devices=$state<MediaDeviceInfo[]>([]),loading=$state(false),granting=$state(false),error=$state(''),defaultName=$state('');
  let alive=true;
  const selected=$derived(devices.find(d=>d.deviceId===$preferences.microphoneId));
  const labelsHidden=$derived(!devices.length||devices.every(d=>!d.label));
  async function refresh(){
    if(loading||!navigator.mediaDevices?.enumerateDevices)return;loading=true;
    try{const next=await navigator.mediaDevices.enumerateDevices();if(alive){devices=next.filter(d=>d.kind==='audioinput'&&d.deviceId&&d.deviceId!=='default');defaultName=next.find(d=>d.kind==='audioinput'&&d.deviceId==='default')?.label??'';}}
    catch{if(alive)error='Microphones could not be listed. Check microphone access and try again.';}
    finally{if(alive)loading=false;}
  }
  async function allowMicrophone(){
    if(granting)return;granting=true;error='';let stream:MediaStream|null=null;
    try{stream=await navigator.mediaDevices.getUserMedia(microphoneConstraints($preferences.microphoneId));await refresh();}
    catch(cause){if(alive)error=dictationStartupMessage(cause,'microphone');}
    finally{stream?.getTracks().forEach(track=>track.stop());if(alive)granting=false;}
  }
  function microphoneChanged(event:Event){
    const input=event.currentTarget as HTMLSelectElement;error='';
    try{savePreferences({microphoneId:input.value});}catch{input.value=$preferences.microphoneId;error='Could not save the microphone setting. Try again.';}
  }
  function themeChanged(event:Event){
    const input=event.currentTarget as HTMLSelectElement;error='';
    try{savePreferences({theme:input.value as 'system'|'light'|'dark'});}catch{input.value=$preferences.theme;error='Could not save the theme setting. Try again.';}
  }
  onMount(()=>{void refresh();const changed=()=>void refresh();navigator.mediaDevices?.addEventListener('devicechange',changed);return()=>{alive=false;navigator.mediaDevices?.removeEventListener('devicechange',changed);};});
</script>

<section class="card general-settings" aria-label="General settings">
  <h2>General</h2>
  <div class="setting-row"><div><label for="studio-microphone">Microphone</label><p>{selected?.label||($preferences.microphoneId?'Selected microphone':defaultName||'System default (recommended)')}</p><small>Used for your next recording.</small></div>
    <div class="setting-control"><select id="studio-microphone" aria-label="Microphone" value={$preferences.microphoneId} onchange={microphoneChanged} disabled={granting}><option value="">System default (recommended)</option>{#if $preferences.microphoneId&&!selected}<option value={$preferences.microphoneId}>Previously selected microphone · unavailable</option>{/if}{#each devices as device,i}<option value={device.deviceId}>{device.label||`Microphone ${i+1}`}</option>{/each}</select>
    {#if navigator.mediaDevices}<button class="settings-link" disabled={loading||granting} onclick={labelsHidden?allowMicrophone:refresh}>{granting?'Checking microphone access…':labelsHidden?'Allow microphone access':loading?'Checking microphones…':'Refresh microphones'}</button>{:else}<small>Microphone access is unavailable in this window.</small>{/if}</div>
  </div>
  <FnShortcutSettings/>
  <NoiseReductionControl details/>
  <div class="setting-row"><div><label for="studio-theme">Theme</label><p>Choose the appearance of the app.</p></div><div class="setting-control"><select id="studio-theme" aria-label="Theme" value={$preferences.theme} onchange={themeChanged}><option value="system">System</option><option value="light">Light</option><option value="dark">Dark</option></select></div></div>
  {#if error}<p class="settings-error" role="alert">{error}</p>{/if}
  <BugReport/>
</section>

<style>
.general-settings h2{font-size:21px;margin:0 0 8px}.setting-row{display:grid;grid-template-columns:minmax(0,1fr) minmax(200px,320px);align-items:center;gap:28px;padding:25px 0}.setting-row+.setting-row{border-top:1px solid var(--studio-tone-293449)}.setting-row label{font-size:15px;font-weight:600;color:inherit;margin:0 0 7px}.setting-row p{margin:0;font-size:13px;color:var(--studio-muted)}.setting-row small{display:block;font-size:11px;color:var(--studio-muted);margin-top:7px}.setting-control select{width:100%;padding:11px 14px;border:1px solid var(--studio-border);border-radius:10px;background:var(--studio-sidebar);color:var(--studio-text);font:inherit;font-size:13px}.settings-link{display:block;margin:8px 0 0 auto;background:transparent;padding:3px 0;color:var(--studio-tone-7be3bd);font-size:11px;font-weight:500;border-radius:3px}.settings-error{font-size:12px;color:var(--studio-tone-ffb2be);line-height:1.7}.setting-control select:focus-visible{outline:2px solid var(--studio-focus);outline-offset:3px}@media(max-width:750px){.setting-row{grid-template-columns:1fr;gap:16px}.setting-control{max-width:none}}
.settings-link:hover{background:transparent;text-decoration:underline}
</style>
