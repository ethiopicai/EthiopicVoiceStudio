<script lang="ts">
  import {onMount} from 'svelte';
  import WaveformPlayer from './WaveformPlayer.svelte';
  import {cropPcmWav} from './audio-trim.js';
  let {file,src,label,id,disabled=false,busy=$bindable(false),onApply}:{file:File;src:string;label:string;id:string;disabled?:boolean;busy?:boolean;onApply:(value:{file:File;original:File;start:number;end:number;bytes:Uint8Array})=>Promise<void>}=$props();
  let player=$state<WaveformPlayer>(),trimOpen=$state(false),start=$state(0),end=$state(0),duration=$state(0),error=$state('');
  let alive=true;
  const valid=$derived(Number.isFinite(start)&&Number.isFinite(end)&&start>=0&&end>start&&end<=duration+.001);
  $effect(()=>{src;trimOpen=false;start=0;end=0;duration=0;error='';});
  function loaded(value:number){duration=value;if(!trimOpen||end===0)end=value;}
  function select(a:number,b:number){start=a;end=b;error='';}
  function reset(){player?.stop();start=0;end=duration;error='';}
  export function stop(){player?.stop();}
  async function apply(){
    if(!valid||disabled||busy||!player)return;busy=true;error='';player.stop();const original=file;
    try{const cropped=cropPcmWav(new Uint8Array(await original.arrayBuffer()),start,end)??await player.croppedAudio(start,end);
      if(!alive)return;const trimmed=new File([new Uint8Array(cropped.bytes)],`${original.name.replace(/\.[^.]+$/,'').slice(0,180)}-trimmed.wav`,{type:'audio/wav'});
      await onApply({file:trimmed,original,start:cropped.start,end:cropped.end,bytes:cropped.bytes});if(alive)trimOpen=false;
    }catch(cause){if(alive)error=cause instanceof Error?cause.message:'This segment could not be trimmed. Your original recording is unchanged.';}
    finally{busy=false;}
  }
  onMount(()=>()=>{alive=false;});
</script>
<WaveformPlayer bind:this={player} {src} {label} {id} onDuration={loaded} selection={trimOpen&&valid?{start,end}:null} onSelectionChange={select} selectionDisabled={disabled||busy}/>
<div class="trim-tools">
  <button class="secondary" aria-expanded={trimOpen} aria-controls={`${id}-trim`} disabled={disabled||busy||duration<.1} onclick={()=>{player?.stop();trimOpen=!trimOpen;if(trimOpen)reset();}}>{trimOpen?'Cancel trim':'Trim recording'}</button>
  {#if trimOpen}<div id={`${id}-trim`} class="trim-editor" role="region" aria-label="Trim reference recording">
    <p>Drag the yellow handles, or set the start and end. Keep a clear segment of the voice.</p>
    <div class="trim-fields"><label>Start (seconds)<input type="number" min="0" max={Math.max(0,end-.1)} step="0.01" value={start} disabled={disabled||busy} oninput={event=>{player?.stop();start=event.currentTarget.valueAsNumber;}}/></label><label>End (seconds)<input type="number" min={start+.1} max={duration} step="0.01" value={end} disabled={disabled||busy} oninput={event=>{player?.stop();end=event.currentTarget.valueAsNumber;}}/></label><span class="selection-length">{valid?(end-start).toFixed(2):'—'} s selected</span></div>
    {#if !valid}<p role="status">Choose a start before the end, within this recording.</p>{/if}
    <div class="trim-actions"><button class="secondary" disabled={!valid||disabled||busy} onclick={()=>player?.playSpan(start,end)}>Play selection</button><button class="secondary" disabled={disabled||busy} onclick={reset}>Reset selection</button><button disabled={!valid||disabled||busy} onclick={apply}>{busy?'Trimming…':'Use trimmed copy'}</button></div>
    <small>Your original file and saved profiles stay unchanged.</small>
  </div>{/if}
  {#if error}<p class="trim-error" role="alert">{error}</p>{/if}
</div>
<style>
  .trim-tools{margin-top:12px;font-size:12px}.trim-tools button{font-size:11px;padding:8px 12px}.trim-editor{margin-top:12px;padding:14px;border:1px solid var(--studio-border);border-radius:10px;background:var(--studio-sidebar)}.trim-editor p{color:var(--studio-muted);font-size:12px;line-height:1.7;margin:0 0 12px}.trim-fields,.trim-actions{display:flex;align-items:center;flex-wrap:wrap;gap:12px}.trim-fields label{display:flex;flex-direction:column;gap:6px;color:var(--studio-muted);font-size:11px}.trim-fields input{box-sizing:border-box;width:130px;max-width:100%;padding:8px 10px;background:var(--studio-panel);color:var(--studio-text);border:1px solid var(--studio-border);border-radius:8px;font:inherit;font-size:12px}.selection-length{font-variant-numeric:tabular-nums;color:var(--studio-muted);font-size:11px}.trim-actions{margin-top:14px}.trim-editor small{display:block;color:var(--studio-muted);font-size:10px;margin-top:12px}.trim-error{color:var(--studio-tone-ffb2be);font-size:12px;line-height:1.7}.trim-fields input:focus-visible{outline:2px solid var(--studio-focus);outline-offset:3px}
</style>
