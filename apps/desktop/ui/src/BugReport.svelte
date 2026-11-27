<script lang="ts">
  import {invoke} from '@tauri-apps/api/core';
  const desktop='__TAURI_INTERNALS__' in window;
  const formUrl='https://ethiopic.ai/studio/bugreport';
  const browserReport=JSON.stringify({version:'',os:'',arch:''},null,2);
  let busy=$state(false),notice=$state(''),error=$state('');
  async function open(){
    if(busy)return;busy=true;notice='';error='';
    try{
      if(desktop)await invoke('open_bug_report');
      else window.open(formUrl,'_blank','noopener,noreferrer');
      notice='Complete the report in your browser. You can attach screenshots there.';
    }catch{error='Could not open the form. Visit ethiopic.ai/studio/bugreport in your browser.';}
    finally{busy=false;}
  }
  async function copy(){
    if(busy)return;busy=true;notice='';error='';
    try{
      if(desktop&&navigator.platform.toLowerCase().includes('mac'))await invoke('copy_bug_report');
      else await navigator.clipboard.writeText(desktop?await invoke<string>('get_bug_report'):browserReport);
      notice='App details copied. You can paste them into the report form.';
    }catch{error='Could not copy app details. You can still submit the report form.';}
    finally{busy=false;}
  }
</script>

<section class="bug-report" aria-label="Bug reports">
  <div><h3>Report a bug</h3><p>Tell us what went wrong using our online form.</p><small>App version, operating system and selected models are prefilled as JSON. Review the details and add screenshots in your browser.</small></div>
  <div class="report-actions"><button class="secondary" onclick={open} disabled={busy}>Report a bug</button><button class="copy-report" onclick={copy} disabled={busy}>Copy app details</button></div>
  {#if notice}<p class="report-notice" role="status">{notice}</p>{/if}
  {#if error}<p class="report-error" role="alert">{error}</p>{/if}
</section>

<style>
  .bug-report{display:grid;grid-template-columns:minmax(0,1fr) minmax(200px,320px);align-items:center;gap:16px 28px;padding:25px 0 0;border-top:1px solid var(--studio-tone-293449)}h3{font-size:15px;font-weight:600;margin:0 0 7px}p{margin:0;font-size:13px;color:var(--studio-muted);line-height:1.6}small{display:block;font-size:11px;color:var(--studio-muted);margin-top:7px;line-height:1.6}.report-actions{display:flex;flex-direction:column;align-items:stretch;gap:8px}button{font-size:12px;padding:10px 14px}.copy-report{background:transparent;color:var(--studio-accent);font-size:11px;padding:4px}.copy-report:hover{background:transparent;text-decoration:underline}.report-notice,.report-error{grid-column:1/-1;font-size:12px;overflow-wrap:anywhere}.report-error{color:var(--studio-tone-ffb2be)}button:focus-visible{outline:2px solid var(--studio-focus);outline-offset:3px}@media(max-width:750px){.bug-report{grid-template-columns:1fr}.report-actions{align-items:flex-start}}
</style>
