<script lang="ts">
  import { onMount,untrack } from 'svelte';
  import { waveformPeaks, normalizeWaveformPeaks, formatAudioTime, boundedSeek } from './waveform.js';
  import {cropDecodedAudio} from './audio-trim.js';
  let { src, label, id, onExclusivePlay, onPlaybackPosition, regions=[],selection=null,onSelectionChange,onDuration,selectionDisabled=false }: { src:string; label:string; id:string; onExclusivePlay?:(id:string)=>void; onPlaybackPosition?:(seconds:number,playing:boolean)=>void;regions?:{start:number;end:number}[];selection?:{start:number;end:number}|null;onSelectionChange?:(start:number,end:number)=>void;onDuration?:(seconds:number)=>void;selectionDisabled?:boolean } = $props();
  let track=$state<HTMLDivElement>();
  let dragging:'start'|'end'|null=null;
  let media = $state<HTMLAudioElement>();
  let peaks = $state<{min:number;max:number}[]>([]);
  let duration = $state(0), position = $state(0), playing = $state(false), error = $state(''), loading = $state(false);
  let context: AudioContext | null = null, buffer: AudioBuffer | null = null;
  let source: AudioBufferSourceNode | null = null;
  let decodeTask: Promise<AudioBuffer> | null = null;
  let generation = 0, request = 0, frame = 0, spanStart = 0, spanClock = 0;
  let mounted = false;
  let loadedSrc = '';
  $effect(() => { onPlaybackPosition?.(position,playing); });
  $effect(() => { const value=duration,callback=onDuration;untrack(()=>callback?.(value)); });
  export async function croppedAudio(start:number,end:number){const value=await decode();return cropDecodedAudio(Array.from({length:value.numberOfChannels},(_,i)=>value.getChannelData(i)),value.sampleRate,start,end);}
  function moveTrim(edge:'start'|'end',value:number){if(!selection||selectionDisabled||!onSelectionChange)return;stop();const minimum=Math.min(.1,duration);onSelectionChange(edge==='start'?Math.max(0,Math.min(selection.end-minimum,value)):selection.start,edge==='end'?Math.max(selection.start+minimum,Math.min(duration,value)):selection.end);}
  function trimPointer(event:PointerEvent){if(!dragging||!track)return;const rect=track.getBoundingClientRect();if(rect.width)moveTrim(dragging,(event.clientX-rect.left)/rect.width*duration);}
  function startTrim(event:PointerEvent,edge:'start'|'end'){if(event.button!==0||selectionDisabled)return;event.preventDefault();dragging=edge;(event.currentTarget as HTMLElement).setPointerCapture(event.pointerId);trimPointer(event);}
  function trimKey(event:KeyboardEvent,edge:'start'|'end'){if(!selection)return;const step=event.shiftKey?1:.1;let value=selection[edge];if(event.key==='ArrowLeft'||event.key==='ArrowDown')value-=step;else if(event.key==='ArrowRight'||event.key==='ArrowUp')value+=step;else if(event.key==='Home')value=0;else if(event.key==='End')value=duration;else return;event.preventDefault();moveTrim(edge,value);}
  function announce() { onExclusivePlay?.(id); window.dispatchEvent(new CustomEvent('ethiopic-audio-play',{detail:id})); }
  export function stop() {
    request++; cancelAnimationFrame(frame);
    if (source) { const old = source; source = null; try { old.stop(); } catch { /* Already ended. */ } old.disconnect(); }
    media?.pause(); playing = false;
  }
  function reset() { stop(); generation++; buffer = null; peaks = []; duration = 0; position = 0; error = ''; }
  async function decode(): Promise<AudioBuffer> {
    if (!mounted) throw new Error('Player closed');
    if (buffer) return buffer;
    if (decodeTask) { const pending = decodeTask; try { await pending; } catch { /* Retry only for current source. */ } if (decodeTask === pending) decodeTask = null; if (!mounted) throw new Error('Closed'); return decode(); }
    context ??= new AudioContext();
    const token = generation, url = src, ctx = context;
    const task = (async () => {
      const response = await fetch(url);
      if (!response.ok) throw new Error('Cannot read audio');
      const value = await ctx.decodeAudioData(await response.arrayBuffer());
      if (token !== generation || !mounted) throw new Error('Audio changed');
      buffer = value; duration = value.duration;
      peaks = normalizeWaveformPeaks(waveformPeaks(Array.from({length:value.numberOfChannels},(_,channel)=>value.getChannelData(channel)),240));
      return value;
    })();
    decodeTask = task;
    try { return await task; } finally { if (decodeTask === task) decodeTask = null; }
  }
  async function loadWaveform() {
    const token=generation;
    loading = true;
    try { await decode(); } catch { if (mounted && token===generation) error = 'Waveform unavailable. Use the audio controls below. Precise word playback may be unavailable.'; }
    finally { if (mounted && token===generation) loading = false; }
  }
  function animateMedia() {
    if (!mounted || source || !media || media.paused) return;
    position = media.currentTime;
    frame = requestAnimationFrame(animateMedia);
  }
  function animateSpan(end:number) {
    if (!context || !source) return;
    position = Math.min(end, spanStart + context.currentTime - spanClock);
    frame = requestAnimationFrame(()=>animateSpan(end));
  }
  export async function playSpan(start:number,end:number):Promise<void> {
    stop(); const token = request; error = '';
    if (!Number.isFinite(start) || !Number.isFinite(end) || start < 0 || end <= start) { error = 'This word has no usable audio timing.'; return; }
    try {
      context ??= new AudioContext(); await context.resume();
      if (!mounted || token!==request) return;
      const value = await decode();
      if (!mounted || token !== request) return;
      if (end > value.duration) throw new Error('Invalid span');
      announce();
      const node = context.createBufferSource(); node.buffer = value; node.connect(context.destination); source = node;
      position = start; spanStart = start; spanClock = context.currentTime; playing = true;
      node.onended = () => { node.disconnect(); if (source === node) { source = null; position = end; playing = false; cancelAnimationFrame(frame); } };
      node.start(0,start,end-start); animateSpan(end);
    } catch { if (mounted && token === request) error = 'Precise word playback is unavailable. Use full-recording playback below.'; }
  }
  async function toggle() {
    if (playing) { stop(); return; }
    stop(); if (!media) return;
    media.currentTime = position >= duration ? 0 : boundedSeek(position,duration);
    announce();
    try { await media.play(); } catch { error = 'Playback could not start. Try the audio controls below.'; }
  }
  function mediaStarted() {
    request++; cancelAnimationFrame(frame);
    if (source) { const old = source; source = null; try { old.stop(); } catch { /* Already ended. */ } old.disconnect(); }
    announce(); position=media?.currentTime??position; playing = true;
    frame=requestAnimationFrame(animateMedia);
  }
  function seek(value:number) { stop(); position = boundedSeek(value,duration); if (media) media.currentTime = position; }
  $effect(() => { const next = src; if (mounted && next !== loadedSrc) { reset(); loadedSrc = next; void loadWaveform(); } });
  onMount(()=>{
    mounted = true; loadedSrc = src; void loadWaveform();
    const other = (event:Event) => { if ((event as CustomEvent<string>).detail !== id) stop(); };
    window.addEventListener('ethiopic-audio-play',other);
    return ()=>{ mounted = false; reset();media?.removeAttribute('src');media?.load(); window.removeEventListener('ethiopic-audio-play',other); if(context) void context.close().catch(()=>{}); context = null; };
  });
</script>
<div class="waveform-player" aria-label={label}>
  <button class="play-toggle" onclick={toggle} aria-label={playing ? `Pause ${label}` : `Play ${label}`} disabled={!src}><svg viewBox="0 0 20 20" aria-hidden="true">{#if playing}<path d="M5 4h3v12H5zm7 0h3v12h-3z"/>{:else}<path d="m6 3 11 7-11 7z"/>{/if}</svg></button>
  <div class="waveform-body">
    <div class="waveform-track" bind:this={track}>
      {#if peaks.length}<svg viewBox="0 0 240 48" preserveAspectRatio="none" aria-hidden="true">{#each regions as region}{#if Number.isFinite(region.start)&&Number.isFinite(region.end)&&duration>0}<rect class="edit-region" x={Math.max(0,region.start)/duration*240} y="0" width={Math.max(1,(Math.min(duration,region.end)-Math.max(0,region.start))/duration*240)} height="48"/>{/if}{/each}{#each peaks as peak,i}<line x1={(i+.5)*240/peaks.length} x2={(i+.5)*240/peaks.length} y1={24-peak.max*23} y2={24-peak.min*23} class:played={i/peaks.length <= position/(duration || 1)}/>{/each}<line class="playhead" x1={position/(duration || 1)*240} x2={position/(duration || 1)*240} y1="0" y2="48"/></svg>{:else}<span class="waveform-message">{loading ? 'Reading waveform…' : 'Waveform unavailable'}</span>{/if}
      <input type="range" min="0" max={duration || 0} step="0.01" value={position} disabled={!duration} oninput={(event)=>seek(Number(event.currentTarget.value))} aria-label={`Seek ${label}`} aria-valuetext={`${formatAudioTime(position)} of ${formatAudioTime(duration)}`}/>
      {#if selection&&duration>0}<div class="trim-selection" style:left={`${selection.start/duration*100}%`} style:width={`${(selection.end-selection.start)/duration*100}%`} aria-hidden="true"></div>
        {#each ['start','end'] as edge}{@const boundary=edge as 'start'|'end'}<button type="button" class="trim-handle" style:left={`${selection[boundary]/duration*100}%`} role="slider" aria-label={`Trim ${edge}`} aria-orientation="horizontal" aria-valuemin={boundary==='start'?0:selection.start+Math.min(.1,duration)} aria-valuemax={boundary==='end'?duration:selection.end-Math.min(.1,duration)} aria-valuenow={selection[boundary]} aria-valuetext={`${selection[boundary].toFixed(2)} seconds`} disabled={selectionDisabled} onpointerdown={event=>startTrim(event,boundary)} onpointermove={trimPointer} onpointerup={()=>dragging=null} onpointercancel={()=>dragging=null} onlostpointercapture={()=>dragging=null} onkeydown={event=>trimKey(event,boundary)}><span aria-hidden="true"></span></button>{/each}
      {/if}
    </div>
    <div class="time-label"><span>{formatAudioTime(position)}</span><span aria-hidden="true">/</span><span>{formatAudioTime(duration)}</span></div>
  </div>
  <audio bind:this={media} {src} controls={!!error} preload="metadata" onloadedmetadata={()=>{ if(!buffer && media && Number.isFinite(media.duration)) duration=media.duration; }} onplay={mediaStarted} onpause={()=>{ if(!source) {playing=false;cancelAnimationFrame(frame);} }} ontimeupdate={()=>{ if(media && !source) position=media.currentTime; }} onended={()=>playing=false} onerror={()=>error='This recording could not be played. Check that its audio format is supported.'}><track kind="captions"/></audio>
  {#if error}<p class="waveform-error" role="status">{error}</p>{/if}
</div>
<style>
  .trim-selection{position:absolute;top:0;bottom:0;background:var(--studio-accent);opacity:.16;pointer-events:none;border:1px solid var(--studio-accent)}.trim-handle{position:absolute;top:-4px;bottom:-4px;transform:translateX(-50%);width:18px;min-width:18px;padding:0;z-index:2;touch-action:none;background:transparent;border:0;cursor:ew-resize;display:grid;place-items:center}.trim-handle span{display:block;width:5px;height:100%;border-radius:4px;background:var(--studio-gold);box-shadow:0 0 0 1px var(--studio-border)}.trim-handle:focus-visible{outline:2px solid var(--studio-focus);outline-offset:1px}.trim-handle:disabled{cursor:default;opacity:.6}
  .waveform-track rect.edit-region{fill:var(--studio-gold);fill-opacity:.25;stroke:var(--studio-gold);stroke-width:.4}
  .play-toggle{display:grid;place-items:center}.play-toggle svg{width:17px;height:17px;fill:currentColor}
  .waveform-player{display:flex;align-items:center;flex-wrap:wrap;gap:12px;padding:12px 15px;background:var(--studio-tone-0b1425);border:1px solid var(--studio-tone-26364a);border-radius:12px}.play-toggle{flex:0 0 34px;width:34px;height:34px;padding:0;border-radius:50%;border:1px solid var(--studio-accent);background:var(--studio-tone-12392e);color:var(--studio-tone-8ce7c1);cursor:pointer;font-size:15px}.play-toggle:focus-visible{outline:2px solid var(--studio-gold);outline-offset:3px}.waveform-body{display:flex;align-items:center;gap:12px;flex:1;min-width:80px}.waveform-track{position:relative;height:43px;flex:1;min-width:0}.waveform-track svg{display:block;width:100%;height:100%;overflow:hidden}.waveform-track line{stroke:var(--studio-tone-386355);stroke-width:1;stroke-linecap:round}.waveform-track line.played{stroke:var(--studio-accent)}.waveform-track line.playhead{stroke:var(--studio-gold);stroke-width:.8}.waveform-track input{position:absolute;inset:0;width:100%;height:100%;margin:0;opacity:0;cursor:pointer}.waveform-track:focus-within{outline:2px solid var(--studio-focus);outline-offset:3px;border-radius:4px}.waveform-message{font-size:11px;color:var(--studio-muted);display:flex;align-items:center;height:100%}.time-label{display:flex;justify-content:center;gap:5px;flex-shrink:0;white-space:nowrap;border-left:1px solid var(--studio-border);padding-left:12px;font-size:11px;font-variant-numeric:tabular-nums;color:var(--studio-muted)}.waveform-error{flex-basis:100%;font-size:12px;color:var(--studio-tone-efd291);margin:0}.waveform-player audio{width:100%;max-height:40px}.waveform-player audio:not([controls]){display:none}
</style>
