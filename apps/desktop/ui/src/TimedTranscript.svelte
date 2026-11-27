<script lang="ts">
  import { tick } from 'svelte';
  import type { TimedTextToken } from './timed-text-types';
  import { timedTokenIndexAt, validTimedSequence } from './timed-text.js';
  let {tokens,position=0,playing=false,highlight=true,enabled=true,follow=true,selectedIndex=null,disabled=false,text='',label='Transcript text',language,onSelect}: {
    tokens:readonly TimedTextToken[];position?:number;playing?:boolean;highlight?:boolean;enabled?:boolean;follow?:boolean;
    selectedIndex?:number|null;disabled?:boolean;text?:string;label?:string;language?:string;
    onSelect?:(token:TimedTextToken,index:number)=>void;
  }=$props();
  let region=$state<HTMLDivElement>();
  const timingsValid=$derived(validTimedSequence(tokens));
  const activeIndex=$derived(highlight&&enabled&&playing&&timingsValid?timedTokenIndexAt(tokens,position):null);
  function timingLabel(token:TimedTextToken){return Number.isFinite(token.start)&&Number.isFinite(token.end)?`${token.start.toFixed(2)}–${token.end.toFixed(2)} seconds`:undefined;}
  $effect(()=>{
    const index=activeIndex,container=region,shouldFollow=follow;
    if(index===null||!container||!shouldFollow)return;
    let current=true;
    void tick().then(()=>{
      if(!current)return;
      // Query by a numeric index, never by an untrusted external token ID.
      const word=container.querySelector<HTMLElement>(`[data-word-index="${index}"]`);
      if(!word)return;
      const top=word.offsetTop,bottom=top+word.offsetHeight;
      if(top<container.scrollTop)container.scrollTop=top;
      else if(bottom>container.scrollTop+container.clientHeight)container.scrollTop=bottom-container.clientHeight;
    });
    return()=>{current=false;};
  });
</script>
<!-- svelte-ignore a11y_no_noninteractive_tabindex (Keyboard focus enables scrolling this transcript without moving focus as playback follows words.) -->
<div class="timed-transcript card-transcript-text" bind:this={region} role="region" aria-label={label} tabindex="0" lang={language}>
  {#each tokens as token,index}
    {#if onSelect}<button class="transcript-word" data-word-id={token.id??index} data-word-index={index} class:playback-word={activeIndex===index} class:corrected={token.changed===true} class:selected={selectedIndex===index} aria-current={activeIndex===index?'true':undefined} {disabled} onclick={()=>onSelect?.(token,index)} title={timingLabel(token)}>{token.text}</button>
    {:else}<span class="transcript-word" data-word-id={token.id??index} data-word-index={index} class:playback-word={activeIndex===index} class:corrected={token.changed===true} aria-current={activeIndex===index?'true':undefined} title={timingLabel(token)}>{token.text}</span>{/if}
  {/each}
  {#if !tokens.length}<p>{text||'No transcript available.'}</p>{/if}
</div>
<style>
.timed-transcript{position:relative;display:flex;align-items:baseline;align-content:flex-start;flex-wrap:wrap;gap:2px 5px;max-height:var(--timed-transcript-max-height,360px);overflow:auto;scrollbar-gutter:stable;margin:var(--timed-transcript-margin,16px 0 18px);font-size:var(--timed-transcript-font-size,21px);line-height:1.85;color:inherit;overflow-wrap:anywhere;white-space:normal}.timed-transcript p{margin:0}.timed-transcript .transcript-word{display:inline;border:1px solid transparent;border-radius:5px;padding:0 2px;background:transparent;color:inherit;font:inherit;line-height:inherit;font-weight:400;max-width:100%;overflow-wrap:anywhere;white-space:normal}.timed-transcript button.transcript-word{cursor:pointer}.timed-transcript button.transcript-word:hover,.timed-transcript .transcript-word.selected{background:var(--studio-tone-1c2e45);border-color:var(--studio-tone-40566e)}.timed-transcript .transcript-word.corrected{box-shadow:inset 0 -1px var(--studio-tone-5c9a85)}.timed-transcript .transcript-word.playback-word{background:var(--timed-transcript-highlight,var(--studio-gold));color:var(--timed-transcript-highlight-text,var(--studio-tone-101724));border-color:var(--timed-transcript-highlight,var(--studio-gold));box-shadow:inset 0 -2px var(--studio-tone-c2830c)}.timed-transcript button:disabled{opacity:.6;cursor:default}.timed-transcript:focus-visible,.timed-transcript button:focus-visible{outline:2px solid var(--studio-gold);outline-offset:3px}.timed-transcript:focus-visible{outline-offset:2px}@media(max-width:650px){.timed-transcript{font-size:var(--timed-transcript-font-size,20px)}}
</style>
