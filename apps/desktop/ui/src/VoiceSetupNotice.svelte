<script lang="ts">
  import {downloadSize} from './download-review.js';
  let {notice,onOpen}:{notice:ReturnType<typeof import('./voice-setup.js').voiceSetupNotice>;onOpen:()=>void}=$props();
</script>

<section class="voice-setup-notice" class:needs-attention={notice.phase==='failed'} aria-label="Voice tools setup shortcut">
  <div class="setup-symbol" aria-hidden="true"><svg viewBox="0 0 24 24"><path d="M12 3v11m-4-4 4 4 4-4M4 15v5h16v-5"/></svg></div>
  <div class="setup-copy"><span class="setup-label">VOICE TOOLS · SETUP</span><h2>{notice.title}</h2><p role="status">{notice.description}</p>
    {#if notice.totalBytes>0&&['active','paused','failed'].includes(notice.phase)}
      <div class="setup-numbers"><span>{notice.working?'Finishing setup…':`${notice.percent}% downloaded`}</span><span>{downloadSize(notice.downloadedBytes)} / {downloadSize(notice.totalBytes)}</span></div>
      {#if notice.working}<progress max="100" aria-label="Voice setup preparation progress"></progress>
      {:else}<progress max="100" value={notice.percent??0} aria-label="Voice setup download progress"></progress>{/if}
    {/if}
    <small>Downloads start only after you review the size and approve.</small>
  </div>
  <button onclick={onOpen}>{notice.action}<span aria-hidden="true">→</span></button>
</section>

<style>
  .voice-setup-notice{display:flex;align-items:center;gap:22px;padding:26px;margin-bottom:26px;border:2px solid var(--studio-accent);border-left:6px solid var(--studio-gold);border-radius:18px;background:linear-gradient(110deg,var(--studio-tone-112b2c),var(--studio-panel));box-shadow:0 8px 28px #0001}.setup-symbol{display:grid;place-items:center;width:58px;height:58px;flex:none;border-radius:16px;color:var(--studio-accent);background:var(--studio-page)}svg{width:29px;height:29px;fill:none;stroke:currentColor;stroke-width:1.8;stroke-linecap:round;stroke-linejoin:round}.setup-copy{flex:1;min-width:0}.setup-label{font-size:10px;letter-spacing:1.6px;font-weight:700;color:var(--studio-accent)}h2{font-size:22px;line-height:1.3;margin:9px 0}p{font-size:13px;line-height:1.7;color:var(--studio-muted);margin:0 0 9px;overflow-wrap:anywhere}small{font-size:11px;color:var(--studio-muted)}button{display:flex;align-items:center;justify-content:center;gap:18px;flex:none;padding:13px 18px;font-size:13px}.setup-numbers{display:flex;justify-content:space-between;gap:12px;font-size:11px;color:var(--studio-muted);margin:12px 0 7px}progress{display:block;width:100%;height:8px;accent-color:var(--studio-accent);margin-bottom:12px}.needs-attention{border-color:var(--studio-gold)}@media(max-width:900px){.voice-setup-notice{flex-wrap:wrap;padding:22px;gap:16px}.setup-symbol{width:44px;height:44px}h2{font-size:19px}.voice-setup-notice button{margin-left:60px}}@media(max-width:600px){.setup-symbol{display:none}.voice-setup-notice button{margin-left:0;width:100%}.setup-copy{flex-basis:100%}}
</style>
