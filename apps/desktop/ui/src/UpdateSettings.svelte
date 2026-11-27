<script lang="ts">
 import {appUpdates,updateActionError,checkAppUpdate,saveUpdatePreferences,openAppUpdate,refreshUpdateStatus} from './app-updates';
 let saving=$state(false),manuallyChecked=$state(false);
 function date(value:number|null){return value?new Date(value*1000).toLocaleString():'Never';}
 async function preference(event:Event,key:'automaticChecks'|'usageStatistics'){
  if(!$appUpdates||saving)return;
  const input=event.currentTarget as HTMLInputElement,checked=input.checked;
  // Saved native preferences remain the source of truth if persistence fails.
  input.checked=$appUpdates[key];saving=true;
  try{await saveUpdatePreferences(key==='automaticChecks'?checked:$appUpdates.automaticChecks,key==='usageStatistics'?checked:$appUpdates.usageStatistics);}
  finally{saving=false;}
 }
 async function check(){manuallyChecked=true;await checkAppUpdate(true);}
</script>
<section class="card update-settings" id="app-updates" aria-label="App updates" tabindex="-1">
 <div class="heading"><div><h2>Updates</h2>{#if $appUpdates}<p>Version {$appUpdates.currentVersion} · Preview</p>{/if}</div><button class="secondary" disabled={$appUpdates?.checking||saving} onclick={$appUpdates?check:refreshUpdateStatus}>{$appUpdates?.checking?'Checking…':'Check now'}</button></div>
 {#if $appUpdates}
  <label><input type="checkbox" checked={$appUpdates.automaticChecks} disabled={saving} onchange={event=>preference(event,'automaticChecks')}/><span>Check automatically when online<small>Once a day. You choose when to download and install.</small></span></label>
  <label><input type="checkbox" checked={$appUpdates.usageStatistics} disabled={saving} onchange={event=>preference(event,'usageStatistics')}/><span>Share basic usage statistics<small>Send a random installation ID with update checks, along with app version and platform. Recordings and transcript text stay on your computer.</small></span></label>
  <dl><div><dt>Last checked</dt><dd>{date($appUpdates.lastCheckedAt)}</dd></div><div><dt>Last successful check</dt><dd>{date($appUpdates.lastSuccessfulCheckAt)}</dd></div></dl>
  {#if $appUpdates.availableUpdate}<div class="release"><strong>Version {$appUpdates.availableUpdate.version} is available</strong>{#if $appUpdates.availableUpdate.notes}<p class="notes">{$appUpdates.availableUpdate.notes}</p>{/if}<button onclick={openAppUpdate}>View update</button></div>
  {:else if manuallyChecked&&$appUpdates.lastSuccessfulCheckAt&&!$appUpdates.error&&!$appUpdates.checking}<p role="status">No newer release is available for this computer.</p>{/if}
  {#if $appUpdates.error}<p class="error" role="status">{$appUpdates.error}</p>{/if}
 {/if}
 {#if $updateActionError}<p class="error" role="alert">{$updateActionError}</p>{/if}
</section>
<style>
 .update-settings h2{margin:0;font-size:21px}.heading{display:flex;align-items:center;justify-content:space-between;gap:16px}.heading p{color:var(--studio-muted);font-size:12px;margin:5px 0}.update-settings label{display:flex;align-items:flex-start;gap:12px;margin:24px 0;font-size:14px;font-weight:500}.update-settings input{width:18px;height:18px;flex:none;margin-top:3px;accent-color:var(--studio-accent)}small{display:block;max-width:650px;color:var(--studio-muted);font-size:12px;line-height:1.7;font-weight:400;margin-top:5px}dl{font-size:12px;color:var(--studio-muted)}dl div{display:flex;flex-wrap:wrap;justify-content:space-between;gap:8px;margin:8px 0}dd{margin:0}.release{border-top:1px solid var(--studio-border);padding-top:18px;font-size:14px}.notes{white-space:pre-wrap;overflow-wrap:anywhere;font-size:13px}.release button,.heading button{padding:9px 16px;font-size:12px}.error{color:var(--studio-tone-ffb2be);font-size:12px}section:focus{outline:none}
</style>
