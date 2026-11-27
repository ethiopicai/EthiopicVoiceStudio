<script lang="ts">
 import {appUpdates,dismissAppUpdate,openAppUpdate} from './app-updates';
 let {onSettings,home=false}:{onSettings:()=>void;home?:boolean}=$props();
</script>
{#if $appUpdates?.availableUpdate&&(home||$appUpdates.showNotification)}
 <div class="update-notice" class:home role="status" aria-label={home?'Home app update':'App update available'}>
  <span><strong>{home?'A new version is available':'Update available'}</strong> · {$appUpdates.availableUpdate.version}</span>
  <div><button class="secondary" onclick={openAppUpdate}>View update</button><button class="secondary" onclick={onSettings}>Details</button>{#if !home}<button class="secondary" onclick={dismissAppUpdate} aria-label="Dismiss update notification">Later</button>{/if}</div>
 </div>
{/if}
<style>
 .update-notice{display:flex;flex-wrap:wrap;align-items:center;justify-content:space-between;gap:12px;padding:12px 24px;border-bottom:1px solid var(--studio-border);background:var(--studio-sidebar);font-size:13px;flex-shrink:0}.update-notice.home{border:1px solid var(--studio-border);border-radius:14px;margin-bottom:20px;padding:18px}.update-notice strong{color:var(--studio-accent)}.update-notice div{display:flex;gap:8px}.update-notice button{padding:6px 12px;font-size:12px}
</style>
