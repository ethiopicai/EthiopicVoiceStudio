<script lang="ts">
 import {version as bundledVersion} from '../../src-tauri/tauri.conf.json';
 import {appUpdates,updateActionError,checkAppUpdate} from './app-updates';
 let {onDetails}:{onDetails:()=>void}=$props();
 let manual=$state(false);
 const desktop='__TAURI_INTERNALS__' in window;
 const version=$derived($appUpdates?.currentVersion??bundledVersion);
 const failed=$derived(manual&&!!($appUpdates?.error||$updateActionError));
 async function check(){manual=true;await checkAppUpdate(true);}
</script>

<div class="sidebar-updates" aria-label="Version and updates">
 <button class="app-version" aria-label={`Version ${version}. Open update settings`} title={`Version ${version} · Update settings`} onclick={onDetails}>v{version}</button>
 <button class="update-check" aria-label="Check for updates" title="Check for updates" disabled={!desktop||$appUpdates?.checking} onclick={check}>
  <svg class:checking={$appUpdates?.checking} viewBox="0 0 24 24" aria-hidden="true"><path d="M20 7v5h-5M4 17v-5h5M5.1 8a7.5 7.5 0 0 1 12.4-2L20 9M4 15l2.5 3A7.5 7.5 0 0 0 18.9 16"/></svg>
  <span>{$appUpdates?.checking?'Checking…':'Check for updates'}</span>
 </button>
 {#if !$appUpdates?.checking}
  {#if failed}<button class="check-result error" onclick={onDetails} aria-label="Update check failed. Open update settings" title="Could not check for updates. Open update settings.">Could not check</button>
  {:else if $appUpdates?.availableUpdate}<button class="check-result available" onclick={onDetails} aria-label="Update available. Open update settings" title={`Version ${$appUpdates.availableUpdate.version} is available`}>Update available</button>
  {:else if manual&&$appUpdates?.lastSuccessfulCheckAt}<span class="check-result" role="status">Up to date</span>{/if}
 {/if}
</div>

<style>
 .sidebar-updates{display:flex;flex-direction:column;align-items:flex-start;gap:5px;padding:12px 4px 14px;border-top:1px solid var(--studio-divider);font-size:11px;width:100%}.sidebar-updates button{background:transparent;font-weight:500;border-radius:6px;padding:4px 2px;color:var(--studio-muted);line-height:1.5;max-width:100%}.sidebar-updates button:hover{color:var(--studio-text);background:transparent}.app-version{font-variant-numeric:tabular-nums;font-size:11px;overflow-wrap:anywhere;text-align:left}.update-check{display:flex;align-items:center;gap:8px;font-size:11px;min-height:28px}.update-check svg{width:16px;height:16px;flex:none;fill:none;stroke:currentColor;stroke-width:1.6;stroke-linecap:round;stroke-linejoin:round}.sidebar-updates .check-result{font-size:10px;color:var(--studio-muted);padding:2px;overflow-wrap:anywhere}.sidebar-updates .available{color:var(--studio-accent)}.sidebar-updates .error{color:var(--studio-tone-ffb2be)}.checking{animation:turn 1.4s linear infinite}@keyframes turn{to{transform:rotate(360deg)}}:global(.menu-collapsed) .sidebar-updates{align-items:center;padding:10px 0;text-align:center}:global(.menu-collapsed) .app-version{font-size:10px;text-align:center}:global(.menu-collapsed) .update-check{width:30px;justify-content:center}:global(.menu-collapsed) .update-check span{display:none}:global(.menu-collapsed) .check-result{font-size:9px;line-height:1.5}@media(prefers-reduced-motion:reduce){.checking{animation:none}}
</style>
