<script lang="ts">
 let {prepared,disabled=false,onApply}:{prepared:ReturnType<typeof import('./speech-text.js').prepareSpeechText>;disabled?:boolean;onApply?:()=>void}=$props();
</script>
{#if prepared.originalText.trim()}
 <section class="speech-text-check" aria-label="Speech text check">
  {#if prepared.issues.length}<div class="text-issues" role="alert">{#each prepared.issues as issue}<p>{issue}</p>{/each}</div>
  {:else}<details><summary>Show text used for speech</summary><p class="processing-note">{prepared.hasNumbers?'Numbers written out. ':''}{prepared.hasAbbreviations?'Abbreviations expanded. ':''}This is the exact text sent to the voice model.</p><p class="prepared-script" lang="am">{prepared.text}</p>{#if prepared.changed&&onApply}<button class="secondary" {disabled} onclick={onApply}>Use prepared text</button>{/if}</details>{/if}
  {#each prepared.warnings as warning}<p class="text-warning">{warning}</p>{/each}
 </section>
{/if}
<style>
 .speech-text-check{margin-top:12px;font-size:11px;color:var(--studio-muted);line-height:1.7}.speech-text-check summary{cursor:pointer;color:var(--studio-accent)}.processing-note{margin:8px 0}.prepared-script{max-height:180px;overflow-y:auto;white-space:pre-wrap;overflow-wrap:anywhere;font-size:17px;line-height:1.8;margin:10px 0;padding:12px;border:1px solid var(--studio-border);border-radius:10px;background:var(--studio-page);color:var(--studio-text)}.speech-text-check button{font-size:11px;padding:7px 11px}.text-issues,.text-warning{margin:8px 0;color:var(--studio-gold)}.text-issues p{margin:0 0 5px}
</style>
