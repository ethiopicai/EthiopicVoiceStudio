<script lang="ts">
 import {onMount,tick} from 'svelte';
 import {invoke} from '@tauri-apps/api/core';
 let {onReview,onFirstRun=()=>{},onLater=()=>{}}:{onReview:()=>void;onFirstRun?:()=>void;onLater?:()=>void}=$props();
 let dialog=$state<HTMLDialogElement>();let alive=true;
 async function finish(review:boolean){dialog?.close();try{await invoke('acknowledge_setup_introduction');}catch{/* A failed preference write must not prevent use of the app. */}if(alive){if(review)onReview();else onLater();}}
 onMount(()=>{if('__TAURI_INTERNALS__' in window)void invoke<boolean>('get_setup_introduction').then(async show=>{if(show===true&&alive){onFirstRun();await tick();if(alive)dialog?.showModal();}}).catch(()=>{});return()=>{alive=false;};});
</script>
<dialog bind:this={dialog} aria-labelledby="setup-introduction-title" oncancel={event=>{event.preventDefault();void finish(false);}}>
 <h2 id="setup-introduction-title">Set up your voice tools</h2>
 <p>The app is installed. To use local voice tools, download the models and speech engine selected for your computer.</p>
 <p>Before each download, we’ll show what it contains and its size, and ask for your permission. Voice tools only download after you choose <strong>Download and set up</strong>.</p>
 <p>You can do this later in Settings. Once the resources are installed, supported tools work offline.</p>
 <div><button class="secondary" onclick={()=>void finish(false)}>Not now</button><button onclick={()=>void finish(true)}>Review downloads</button></div>
</dialog>
<style>
 dialog{width:min(500px,calc(100vw - 40px));box-sizing:border-box;background:var(--studio-panel);color:var(--studio-text);border:1px solid var(--studio-border);border-radius:18px;padding:28px}dialog::backdrop{background:#0009}h2{font-size:22px;margin:0 0 18px}p{font-size:13px;line-height:1.7;color:var(--studio-muted)}div{display:flex;justify-content:flex-end;flex-wrap:wrap;gap:12px;margin-top:24px}button{font-size:12px}
</style>
