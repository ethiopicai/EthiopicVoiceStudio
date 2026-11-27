<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  type Status={state:string;message:string;modelVersion?:string|null;release?:string|null};
  type Task={state:string;stage:string};
  type Model={id:string;family:string;release:string;runtime:string;precision:string;license:string;model_version:string|null};
  let inventory=$state<Model[]>([]),browsed=$state('');
  const families=$derived([...new Set(inventory.map(model=>model.family))]);
  const viewed=$derived(inventory.find(model=>model.id===browsed));
  function browse(value:string){browsed=value;try{localStorage.setItem('ethiopic-studio.model-details',value);}catch{}}
  type AsrRuntime={keepReady:boolean;state:string};
  let asrRuntime=$state<AsrRuntime|null>(null),changing=$state(false),preferenceError=$state('');
  let preferenceRevision=0;
  let keepReady=$state(true),asrActive=$state(false);
  $effect(()=>{keepReady=asrRuntime?.keepReady??true;});
  async function changeKeepReady(value:boolean){
    if(changing)return;changing=true;preferenceError='';preferenceRevision++;
    try{const result=await invoke<AsrRuntime>('set_asr_keep_ready',{keepReady:value});if(alive)asrRuntime=result;}
    catch{if(alive){preferenceError='Could not save this preference. Please try again.';keepReady=asrRuntime?.keepReady??true;}}
    finally{if(alive)changing=false;}
  }
  async function unload(){
    if(changing||asrActive||asrRuntime?.state!=='waiting')return;changing=true;preferenceError='';preferenceRevision++;
    try{const result=await invoke<AsrRuntime>('unload_asr_model');if(alive)asrRuntime=result;}
    catch(cause){if(alive)preferenceError=typeof cause==='string'?cause:'Could not unload voice tools. Finish any current recording and try again.';}
    finally{if(alive)changing=false;}
  }
  let statuses=$state<Record<string,Status>>({asr:{state:'checking',message:''},cleanup:{state:'checking',message:''}});
  let runtime=$state<Record<string,string>>({asr:'Checking…',cleanup:'Checking…'});
  let busy=$state(false),alive=true;
  let pending:Promise<void>|null=null;
  const models=[{id:'asr',title:'Transcription & dictation',name:'Amharic CTC'},{id:'cleanup',title:'Voice Cleanup',name:'NVIDIA NemotronLabs RE-USE'}];
  async function readStatuses(){
    const revision=preferenceRevision;
    const values=await Promise.allSettled([
      invoke<Status>('get_asr_status'),invoke<Status>('get_cleanup_status'),
      invoke<Task|null>('get_active_transcription_job'),invoke<Task|null>('get_active_dictation_session'),invoke<Task|null>('get_active_cleanup_job'),invoke<AsrRuntime>('get_asr_runtime_status'),invoke<{manifest:{variants:Model[];aliases:Record<string,string>}}>('get_studio_model_source')
    ]);
    if(!alive)return;
    if(values[6].status==='fulfilled'){
      const source=(values[6].value as {manifest:{variants:Model[];aliases:Record<string,string>}}).manifest;inventory=source.variants;
      let saved=browsed;try{saved||=localStorage.getItem('ethiopic-studio.model-details')??'';}catch{}
      browse(source.aliases[saved]??saved??'');if(!inventory.some(m=>m.id===browsed))browse(inventory[0]?.id??'');
    }
    for(const [i,id] of ['asr','cleanup'].entries()){
      const value=values[i];statuses[id]=value.status==='fulfilled'?value.value as Status:{state:'unavailable',message:'Could not check these voice tools. Try again.'};
    }
    if(values[5].status==='fulfilled'&&!changing&&revision===preferenceRevision)asrRuntime=values[5].value as AsrRuntime;
    const task=(i:number)=>values[i].status==='fulfilled'?values[i].value as Task|null:null;
    const asr=[task(2),task(3)].find(t=>t&&['running','loading','ready','recording','finishing'].includes(t.state)),cleanup=task(4);
    asrActive=!!asr;
    const resident=asrRuntime?.state==='waiting'?'Loaded · Waiting for recordings':asrRuntime?.state==='running'?'Processing recording':asrRuntime?.keepReady?'Starts on next recording · stays ready':'Not loaded · starts when needed';
    runtime={asr:values[2].status==='rejected'||values[3].status==='rejected'?'Status unavailable':asr?.stage||resident,cleanup:values[4].status==='rejected'?'Status unavailable':cleanup?.stage||'Not loaded · starts when needed'};
  }
  function refresh(){
    // Poll quietly; a manual check joins any request already in progress.
    if(!pending)pending=readStatuses().finally(()=>{pending=null;});
    return pending;
  }
  async function checkAgain(){
    if(busy)return;busy=true;
    try{if('__TAURI_INTERNALS__' in window)await invoke('check_model_health',{manual:true}).catch(()=>{});await refresh();}finally{if(alive)busy=false;}
  }
  onMount(()=>{void refresh();const changed=()=>void refresh();window.addEventListener('studio-tools-changed',changed);const timer=setInterval(()=>void refresh(),24*60*60*1000);return()=>{alive=false;clearInterval(timer);window.removeEventListener('studio-tools-changed',changed);};});
</script>
<section class="card local-models" aria-label="Local models">
  <div class="section-heading"><div><h2>Local models</h2><p>Voice tools cached on this computer.</p></div><button class="secondary" onclick={checkAgain} disabled={busy} aria-busy={busy}>{busy?'Checking…':'Check again'}</button></div>
  <div class="keep-ready">
    <label><input type="checkbox" bind:checked={keepReady} disabled={!asrRuntime||changing} onchange={event=>void changeKeepReady(event.currentTarget.checked)} /> Keep voice tools ready</label>
    <p>Transcription and Dictation share one model. After the first recording it stays loaded for the next job. Unload it to free memory; your next recording loads it again. Turn this option off to release it after every recording.</p>
    <button class="secondary unload-model" disabled={changing||asrActive||asrRuntime?.state!=='waiting'} onclick={unload}>Unload from memory</button>
    {#if asrRuntime?.keepReady}<span>{asrRuntime.state==='waiting'?'Loaded · Waiting for recordings':asrRuntime.state==='running'?'Processing recording':'Will stay ready after your next recording'}</span>{/if}
    {#if preferenceError}<p role="alert">{preferenceError}</p>{/if}
  </div>
  <div class="model-list">{#each models as model}
    {@const status=statuses[model.id]}
    <details class="model-details"><summary><span>{model.title}</span><span class="cache-status" class:ready={status.state==='ready'}>{status.state==='ready'?'Cached · Ready':status.state==='checking'?'Checking…':status.state==='unsupported'?'Unsupported':'Needs attention'}</span></summary>
      <dl><div><dt>Selected model</dt><dd>{status.release??model.name}</dd></div><div><dt>Version</dt><dd>{status.modelVersion??'Unavailable'}</dd></div><div><dt>Local cache</dt><dd>{status.state==='ready'?'Ready for use':status.state==='unsupported'&&status.modelVersion?'Installed · incompatible with this computer':status.state==='checking'?'Checking…':'Setup or repair needed'}</dd></div><div><dt>Runtime</dt><dd>{runtime[model.id]}</dd></div><div><dt>Processing</dt><dd>On this computer</dd></div></dl>
      <p class="model-message">{status.message}</p>
    </details>
  {/each}</div>
  {#if inventory.length}<details class="model-details inventory"><summary>Browse model details</summary>
    <label for="studio-model-details">Model release</label><select id="studio-model-details" value={browsed} onchange={event=>browse(event.currentTarget.value)}>
      {#each families as family}<optgroup label={family}>{#each inventory.filter(model=>model.family===family) as model}<option value={model.id}>{model.release} · {model.precision}</option>{/each}</optgroup>{/each}
    </select>
    {#if viewed}<dl><div><dt>Release</dt><dd>{viewed.release}</dd></div><div><dt>Version</dt><dd>{viewed.model_version??'Third-party codec'}</dd></div><div><dt>Variant</dt><dd>{viewed.id}</dd></div><div><dt>Runtime</dt><dd>{viewed.runtime}</dd></div><div><dt>License</dt><dd>{viewed.license}</dd></div></dl>{/if}
    <p class="model-message">Browse the available releases here. Setup chooses compatible variants for this computer.</p>
  </details>{/if}
</section>
<style>
  .inventory{margin-top:16px}.inventory select{width:100%;max-width:100%;padding:10px;font:inherit;font-size:11px;background:var(--studio-page);color:var(--studio-text);border:1px solid var(--studio-border);border-radius:8px;margin:10px 0 16px}.inventory label{font-size:12px}
  .keep-ready{margin:16px 0;padding:14px;border:1px solid var(--studio-tone-33415a);border-radius:12px}.keep-ready .unload-model{font-size:11px;padding:8px 12px;margin-top:12px}.keep-ready label{display:flex;align-items:center;gap:9px;font-size:13px;cursor:pointer}.keep-ready input{accent-color:var(--studio-tone-2cd4a3)}.keep-ready p{font-size:12px;line-height:1.6;color:var(--studio-tone-94a5c1);margin:9px 0 0}.keep-ready span{display:block;font-size:11px;color:var(--studio-tone-7eddb7);margin-top:8px}
  .local-models .section-heading{justify-content:space-between;gap:20px}.local-models .section-heading button{font-size:11px;padding:7px 12px;flex:none;min-width:94px}.model-list{display:flex;flex-direction:column;gap:12px}.model-details{border:1px solid var(--studio-tone-33415a);background:var(--studio-tone-0c1425);border-radius:12px;padding:0 15px}.model-details summary{display:flex;align-items:center;justify-content:space-between;gap:12px;padding:16px 0;cursor:pointer;font-size:13px;list-style:none}.model-details summary::after{content:'⌄';color:var(--studio-tone-8fa1be)}.model-details[open] summary::after{content:'⌃'}.cache-status{margin-left:auto;font-size:10px;color:var(--studio-tone-e2be7a);white-space:nowrap}.cache-status.ready{color:var(--studio-tone-7eddb7)}.model-details dl{margin:0}.model-details dl div{gap:20px;font-size:12px}.model-details dd{overflow-wrap:anywhere;min-width:0;max-width:65%;color:var(--studio-tone-d4dff2)}.model-message{font-size:11px;line-height:1.7;color:var(--studio-tone-94a5c1)}.model-details summary:focus-visible{outline:2px solid var(--studio-focus);outline-offset:4px}@media(max-width:650px){.model-details summary{flex-wrap:wrap}.cache-status{font-size:9px}}
</style>
