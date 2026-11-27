<script lang="ts">
  import {onMount,type Snippet} from 'svelte';
  let {id,label,storageKey,history,controls,children,open=$bindable(true)}:{id:string;label:string;storageKey:string;history:Snippet;controls?:Snippet;children:Snippet;open?:boolean}=$props();
  let layout=$state<HTMLElement>(),layoutWidth=$state(850),width=$state(260),resizing=$state(false),storageReady=$state(false);
  const maximum=$derived(Math.max(120,Math.min(640,layoutWidth-260-8)));
  const minimum=$derived(Math.min(180,maximum));
  const visibleWidth=$derived(Math.max(minimum,Math.min(maximum,width)));
  $effect(()=>{if(storageReady)try{localStorage.setItem(`${storageKey}.historyOpen`,String(open));}catch{/* Layout remains usable without storage. */}});
  function setWidth(value:number){width=Math.round(Math.max(minimum,Math.min(maximum,value)));}
  function saveWidth(){try{localStorage.setItem(`${storageKey}.historyWidth`,String(width));}catch{/* Keep the current session's width. */}}
  function beginResize(event:PointerEvent){if(event.button!==0)return;event.preventDefault();resizing=true;(event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);}
  function moveResize(event:PointerEvent){if(resizing&&layout)setWidth(event.clientX-layout.getBoundingClientRect().left);}
  function endResize(){if(resizing){resizing=false;saveWidth();}}
  function resizeKey(event:KeyboardEvent){const step=event.shiftKey?50:20;if(event.key==='ArrowLeft')setWidth(visibleWidth-step);else if(event.key==='ArrowRight')setWidth(visibleWidth+step);else if(event.key==='Home')setWidth(minimum);else if(event.key==='End')setWidth(maximum);else return;event.preventDefault();saveWidth();}
  onMount(()=>{
    try{const savedOpen=localStorage.getItem(`${storageKey}.historyOpen`);if(savedOpen!==null)open=savedOpen==='true';const savedWidth=Number(localStorage.getItem(`${storageKey}.historyWidth`));if(Number.isFinite(savedWidth)&&savedWidth>=120&&savedWidth<=1000)width=savedWidth;}catch{/* Keep defaults. */}
    storageReady=true;if(!layout)return;layoutWidth=layout.getBoundingClientRect().width;
    const observer=new ResizeObserver(entries=>layoutWidth=entries[0].contentRect.width);observer.observe(layout);return()=>observer.disconnect();
  });
</script>

<div class="history-layout" bind:this={layout} class:history-hidden={!open} class:resizing style={`--history-width:${visibleWidth}px`}>
  {#if open}
    <section {id} class="history-panel" aria-label={label}>
      <div class="history-heading"><h2>History</h2><button class="secondary icon-button" aria-label={`Hide ${label.toLowerCase()}`} aria-expanded="true" aria-controls={id} onclick={()=>open=false}><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 4h16v16H4V4Zm11 0v16m-4-12-4 4 4 4"/></svg></button></div>
      {#if controls}{@render controls()}{/if}
      <div class="history-scroll">{@render history()}</div>
    </section>
    <!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions (Keyboard-resizable window separator.) -->
    <div class="history-resizer" role="separator" tabindex="0" aria-label={`Resize ${label.toLowerCase()}`} aria-orientation="vertical" aria-controls={id} aria-valuemin={Math.round(minimum)} aria-valuemax={Math.round(maximum)} aria-valuenow={Math.round(visibleWidth)} title="Drag to resize history. Use arrow keys when focused." onpointerdown={beginResize} onpointermove={moveResize} onpointerup={endResize} onpointercancel={endResize} onlostpointercapture={endResize} onkeydown={resizeKey} ondblclick={()=>{setWidth(260);saveWidth();}}></div>
  {/if}
  <div class="history-main">{#if !open}<div class="history-toolbar"><button class="secondary" aria-label={`Show ${label.toLowerCase()}`} aria-expanded="false" aria-controls={id} onclick={()=>open=true}>Show history</button></div>{/if}{@render children()}</div>
</div>

<style>
  .history-layout{display:grid;grid-template-columns:var(--history-width) 8px minmax(0,1fr);width:100%;height:100%;min-height:0;min-width:0;flex:1;overflow:hidden}.history-layout.history-hidden{grid-template-columns:minmax(0,1fr)}.history-panel{display:flex;flex-direction:column;min-height:0;min-width:0;overflow:hidden;background:var(--studio-tone-0b1322)}.history-heading{display:flex;align-items:center;justify-content:space-between;gap:12px;min-height:66px;padding:12px 18px;flex-shrink:0}.history-heading h2{font-size:16px;margin:0;color:var(--studio-text)}.icon-button{width:30px;height:30px;padding:5px;display:grid;place-items:center;flex-shrink:0;border:1px solid var(--studio-border);border-radius:8px;background:transparent}.icon-button svg{width:18px;height:18px;fill:none;stroke:currentColor;stroke-width:1.6}.history-scroll{overflow-y:auto;min-height:0;flex:1;padding:0 10px 16px;scrollbar-gutter:stable}.history-resizer{position:relative;width:8px;min-width:8px;cursor:col-resize;touch-action:none;background:var(--studio-sidebar);border-left:1px solid var(--studio-divider);outline-offset:-2px}.history-resizer::after{content:'';position:absolute;left:2px;top:0;bottom:0;width:2px;border-radius:2px;background:transparent}.history-resizer:hover::after,.history-resizer:focus-visible::after,.resizing .history-resizer::after{background:var(--studio-accent)}.resizing{cursor:col-resize;user-select:none}.history-main{display:flex;flex-direction:column;min-height:0;min-width:0;overflow:hidden}.history-toolbar{flex:none;padding:10px 24px 0}.history-toolbar button{font-size:11px;padding:7px 11px}button:focus-visible,.history-resizer:focus-visible{outline:2px solid var(--studio-focus);outline-offset:2px}.history-resizer:focus-visible{outline-offset:-2px}
</style>
