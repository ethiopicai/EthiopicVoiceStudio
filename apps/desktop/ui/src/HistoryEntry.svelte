<script lang="ts">
  import type {Snippet} from 'svelte';
  import type {HTMLButtonAttributes} from 'svelte/elements';
  let {name,detail,avatar,selected=false,onSelect,checked=false,onChecked,selectionLabel,disabled=false,class:className='',...attributes}:{name:string;detail:string;avatar?:Snippet;selected?:boolean;onSelect:()=>void;checked?:boolean;onChecked?:(checked:boolean)=>void;selectionLabel?:string}&Omit<HTMLButtonAttributes,'name'|'onclick'>=$props();
</script>
<div class="history-entry-row">{#if onChecked}<input type="checkbox" aria-label={selectionLabel??`Select ${name}`} {checked} {disabled} onchange={event=>onChecked?.(event.currentTarget.checked)}/>{/if}<button {...attributes} {disabled} class={`secondary history-entry ${className}`} class:selected class:with-avatar={!!avatar} aria-current={selected?'true':undefined} onclick={onSelect}>{#if avatar}{@render avatar()}<span class="history-copy"><strong>{name}</strong><small>{detail}</small></span>{:else}<strong>{name}</strong><small>{detail}</small>{/if}</button></div>
<style>
  .history-entry.with-avatar{flex-direction:row;align-items:center;gap:9px}.history-copy{min-width:0}.history-copy strong,.history-copy small{display:block}.history-copy small{margin-top:5px}

  .history-entry-row{display:flex;align-items:center;gap:5px;min-width:0}.history-entry-row>input{width:15px;height:15px;flex:none;margin:0 0 0 5px;accent-color:var(--studio-accent);cursor:pointer}.history-entry-row>input:disabled{cursor:default}.history-entry-row>input:focus-visible{outline:2px solid var(--studio-focus);outline-offset:3px}.history-entry-row>.history-entry{flex:1}
  .history-entry{display:flex;flex-direction:column;align-items:flex-start;gap:6px;width:100%;min-width:0;text-align:left;padding:12px;border:1px solid transparent;background:transparent;color:var(--studio-text);font:inherit;border-radius:9px}.history-entry strong{font-size:12px;font-weight:500;max-width:100%;overflow-wrap:anywhere;white-space:normal}.history-entry small{font-size:10px;color:var(--studio-muted)}.history-entry:hover,.history-entry.selected{background:var(--studio-tone-142337);border-color:var(--studio-tone-2b4d50)}.history-entry.selected{color:var(--studio-tone-e4fff3)}.history-entry:disabled{opacity:.55;cursor:default}.history-entry:focus-visible{outline:2px solid var(--studio-focus);outline-offset:2px}
</style>
