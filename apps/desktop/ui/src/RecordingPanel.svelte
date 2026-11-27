<script lang="ts">
  import type {Snippet} from 'svelte';
  let {title,label,actions,children}:{title:string;label?:string;actions?:Snippet;children:Snippet}=$props();
</script>
<section class="recording-panel" aria-label={label??title}>
  <div class="panel-heading"><h2>{title}</h2>{#if actions}<div class="panel-actions">{@render actions()}</div>{/if}</div>
  {@render children()}
</section>
<style>
  .panel-heading{min-height:32px}
  .recording-panel{border:1px solid var(--studio-tone-29364e);border-radius:14px;background:var(--studio-tone-0d1526);padding:20px;min-width:0;flex-shrink:0}.panel-heading{display:flex;align-items:center;justify-content:space-between;gap:14px;flex-wrap:wrap;margin-bottom:18px}.panel-heading h2{font-size:14px;margin:0;font-weight:500;min-width:0;overflow-wrap:anywhere}.panel-actions{display:flex;align-items:center;gap:8px;flex-wrap:wrap}.panel-actions :global(button){font-size:11px;padding:7px 10px}
</style>
