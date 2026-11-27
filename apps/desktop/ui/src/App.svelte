<script lang="ts">
  import { onMount, tick } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import {listen} from '@tauri-apps/api/event';
  import {receiveFnGesture,type FnGesture} from './fn-dictation';
  import Home from './Home.svelte';
  import VoiceCleanup from './VoiceCleanup.svelte';
  import VoiceWorkspace from './VoiceWorkspace.svelte';
  import VoiceEditing from './VoiceEditing.svelte';
  import Transcription from './Transcription.svelte';
  import Dictation from './Dictation.svelte';
  import ModelSettings from './ModelSettings.svelte';
  import ModelLicenses from './ModelLicenses.svelte';
  import ResourceSetup from './ResourceSetup.svelte';
  import HardwareSetup from './HardwareSetup.svelte';
  import DownloadReview from './DownloadReview.svelte';
  import SetupIntroduction from './SetupIntroduction.svelte';
  import {voiceSetupNotice,needsSetupOnLaunch} from './voice-setup.js';
  import GeneralSettings from './GeneralSettings.svelte';
  import UpdateNotice from './UpdateNotice.svelte';
  import UpdateSettings from './UpdateSettings.svelte';
  import SidebarUpdates from './SidebarUpdates.svelte';
  import {initializeAppUpdates} from './app-updates';
  onMount(initializeAppUpdates);
  async function openUpdateSettings(){workspace='setup';await tick();const section=document.getElementById('app-updates');section?.scrollIntoView({block:'start'});section?.focus({preventScroll:true});}
  import {readRecordingDrop,canDropRecording,requestRecordingTransfer} from './recording-transfer.js';
  import {initializePreferences} from './preferences.js';
  import {refreshRecordingTitles} from './recording-titles.js';
  onMount(()=>{if('__TAURI_INTERNALS__' in window)void refreshRecordingTitles();});
  const releasePreferences=initializePreferences();
  onMount(()=>releasePreferences);
  onMount(()=>{
    let active=true,unlisten:(()=>void)|undefined;
    const receive=(value:FnGesture)=>{if(!active)return;if(value.pressed)workspace='dictation';receiveFnGesture(value);};
    if('__TAURI_INTERNALS__' in window)void listen<FnGesture>('dictation-shortcut',event=>receive(event.payload)).then(stop=>{if(!active)stop();else{unlisten=stop;void invoke<{active?:FnGesture}>('get_fn_shortcut_status').then(status=>{if(status.active)receive(status.active);}).catch(()=>{});}}).catch(()=>{});
    return()=>{active=false;unlisten?.();};
  });
  type Summary = { os: string; arch: string; logicalCpus: number; totalMemoryBytes?: number };
  type Workspace = 'home' | 'cleanup' | 'tts' | 'transcription' | 'dictation' | 'infilling' | 'projects' | 'setup';
  let workspace = $state<Workspace>('home');
  async function openDownloadSetup(){workspace='setup';await tick();const section=document.getElementById('voice-tools-setup')??document.querySelector<HTMLElement>('.hardware-setup');section?.scrollIntoView({block:'start'});section?.focus({preventScroll:true});}
  let transferTarget=$state(''),transferError=$state('');
  onMount(()=>{const receive=(event:Event)=>{const requested=(event as CustomEvent).detail?.target;const target=requested==='clone'?'tts':requested;if(['cleanup','infilling','transcription','dictation','tts'].includes(target)){workspace=target;voiceOpen=true;}};window.addEventListener('studio-recording-transfer',receive);return()=>window.removeEventListener('studio-recording-transfer',receive);});
  function recordingDrop(event:DragEvent,target:Workspace){event.preventDefault();event.stopPropagation();transferTarget='';transferError='';try{requestRecordingTransfer(target as import('./recording-transfer.js').RecordingTarget,readRecordingDrop(event.dataTransfer));}catch(error){transferError=error instanceof Error?error.message:'This recording could not be transferred.';}}
  function recordingOver(event:DragEvent,target:Workspace){if(canDropRecording(event.dataTransfer)){event.preventDefault();if(event.dataTransfer)event.dataTransfer.dropEffect='copy';transferTarget=target;}}
  let dictationSettingsOpen=$state(false);
  let deviceStatusOpen=$state(false);
  let menuCollapsed = $state(false);
  let historyOpen = $state(true);
  let voiceOpen = $state(true);
  const tools: { id: Workspace; label: string; description: string; path: string }[] = [
    { id: 'tts', label: 'Text to Speech', description: 'Generate speech with the default voice or your saved reference voices.', path: 'M4 10v4m4-8v12m4-15v18m4-15v12m4-8v4' },
    { id: 'transcription', label: 'Transcription', description: 'Turn an audio recording into editable Amharic text.', path: 'M8 3h7l4 4v14H5V3h3m6 0v5h5M8 12h8m-8 4h6' },
    { id: 'dictation', label: 'Dictation', description: 'Speak to write, right here in your studio.', path: 'M9 5a3 3 0 0 1 6 0v7a3 3 0 0 1-6 0V5m-3 6v1a6 6 0 0 0 12 0v-1m-6 7v4m-4 0h8' },
    { id: 'cleanup', label: 'Voice Cleanup', description: 'Prepare a recording for speech enhancement.', path: 'M3 12h3l3-7 4 14 3-7h5M17 3v4m-2-2h4' },
    { id: 'infilling', label: 'Voice Editing', description: 'Replace a section of speech while keeping the original.', path: 'M3 10v4m4-7v10m4-13v16m5-9h6m-3-3v6' },
  ];
  const current = $derived(tools.find(tool => tool.id === workspace));
  let asrStatus = $state<{state:string;message:string;modelVersion?:string} | null>(null);
  let setupProgress=$state<import('./voice-setup.js').SetupProgress>(null);
  const setupNotice=$derived(voiceSetupNotice(asrStatus,setupProgress));
  let checkingSetup=false,launchChecked=false,startupNavigationCancelled=false;
  async function checkAsr() {
    if(checkingSetup)return;checkingSetup=true;
    try {
      const [status,progress]=await Promise.allSettled([invoke<{state:string;message:string;modelVersion?:string}>('get_asr_status'),invoke<NonNullable<import('./voice-setup.js').SetupProgress>>('get_voice_setup')]);
      asrStatus=status.status==='fulfilled'?status.value:{state:'unknown',message:'Voice tools could not be checked. Please try again.'};
      if(progress.status==='fulfilled'&&typeof progress.value?.state==='string')setupProgress=progress.value;
      if(!launchChecked){launchChecked=true;if(!startupNavigationCancelled&&workspace==='home'&&needsSetupOnLaunch(voiceSetupNotice(asrStatus,setupProgress)))void openDownloadSetup();}
    } finally {checkingSetup=false;}
  }
  let summary = $state<Summary | null>(null);
  let text = $state('ሰላም! ወደ ኢትዮጲክ የድምፅ ስቱዲዮ እንኳን በደህና መጡ።');
  let voiceStyle = $state('preset:default');
  let cleanupRecording=$state<File|null>(null);
  let cleanupJobId=$state('');
  let referenceVoice = $state<File | null>(null);
  let referenceMedia=$state<import('./recording-transfer.js').RecordingContext|null>(null);
  let referenceTranscript=$state('');
  let error = $state('');
  let amharic = $state(false);
  let desktop = $state('__TAURI_INTERNALS__' in window);
  onMount(() => {
    desktop = '__TAURI_INTERNALS__' in window;
    if (!desktop) return;
    const userNavigation=(event:Event)=>{if(event.target instanceof Element&&event.target.closest('.navigation-panel button,.brand'))startupNavigationCancelled=true;};
    document.addEventListener('click',userNavigation,true);
    void checkAsr();
    let active = true;
    invoke<Summary>('system_summary').then(value => { if (active) summary = value; }).catch(() => { if (active) error = 'Computer information is unavailable.'; });
    const changed=()=>{if(active)void checkAsr();};
    window.addEventListener('studio-tools-changed',changed);
    let stopTools:(()=>void)|undefined,stopProgress:(()=>void)|undefined;
    void listen('studio-tools-changed',()=>{window.dispatchEvent(new Event('studio-tools-changed'));}).then(stop=>{if(active)stopTools=stop;else stop();}).catch(()=>{});
    void listen<NonNullable<import('./voice-setup.js').SetupProgress>>('studio-setup-progress',event=>{if(active){setupProgress=event.payload;window.dispatchEvent(new CustomEvent('studio-setup-progress',{detail:event.payload}));if(event.payload.ready)void checkAsr();}}).then(stop=>{if(active)stopProgress=stop;else stop();}).catch(()=>{});
    const timer=window.setInterval(()=>{if(active){void invoke('check_model_health',{manual:false}).catch(()=>{});void checkAsr();}},24*60*60*1000);
    return () => { active = false; window.clearInterval(timer);document.removeEventListener('click',userNavigation,true);window.removeEventListener('studio-tools-changed',changed);stopTools?.();stopProgress?.(); };
  });
</script>

<svelte:window onkeydown={event=>{if(event.key==='Escape')deviceStatusOpen=false;}}/>

<svelte:head><meta name="description" content="EthiopicAI Voice Studio · Local Amharic transcription and dictation"/></svelte:head>
<DownloadReview/>
<SetupIntroduction onReview={openDownloadSetup} onFirstRun={()=>{if(!startupNavigationCancelled&&workspace==='home')void openDownloadSetup();}} onLater={()=>{launchChecked=true;workspace='home';}}/>

<div class="app-shell" class:menu-collapsed={menuCollapsed}>
  <aside class="navigation-panel" aria-label="Studio navigation">
    <div class="navigation-header">
      <a class="brand" href="#welcome" onclick={() => workspace = 'home'}><img class="brand-logo" src="/ethiopic-ai-logo.png" alt=""/><span>Ethiopic<span class="gold">AI</span><small>VOICE STUDIO</small></span></a>
      <button class="menu-toggle secondary" aria-label={menuCollapsed ? 'Expand navigation' : 'Collapse navigation'} aria-expanded={!menuCollapsed} aria-controls="studio-navigation" title={menuCollapsed ? 'Expand navigation' : 'Collapse navigation'} onclick={() => menuCollapsed = !menuCollapsed}><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M3 4h18v16H3V4Zm5 0v16m5-12 4 4-4 4"/></svg></button>
    </div>
    <nav id="studio-navigation" aria-label="Workspaces"><p class="nav-label">YOUR STUDIO</p>
      <button class="nav-item" title="Home" aria-label="Home" class:selected={workspace === 'home'} aria-current={workspace === 'home' ? 'page' : undefined} onclick={() => workspace = 'home'}><svg viewBox="0 0 24 24" aria-hidden="true"><path d="m3 10 9-7 9 7v11h-6v-7H9v7H3V10Z"/></svg><span>Home</span></button>
      <button class="nav-item voice-group" class:group-active={['tts', 'infilling', 'cleanup'].includes(workspace)} aria-expanded={voiceOpen} aria-controls="voice-tools" title="Voice" aria-label="Voice" ondragover={event=>{if(canDropRecording(event.dataTransfer)){event.preventDefault();voiceOpen=true;menuCollapsed=false;}}} onclick={() => { if (menuCollapsed) { menuCollapsed = false; voiceOpen = true; } else { voiceOpen = !voiceOpen; } }}>
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 10v4m4-8v12m4-15v18m4-15v12m4-8v4"/></svg><span>Voice</span><span class="nav-chevron" aria-hidden="true">{voiceOpen ? '⌄' : '›'}</span>
      </button>
      <div id="voice-tools" class="voice-submenu" hidden={!voiceOpen}>
        {#each ['tts', 'infilling', 'cleanup'] as voiceId}
          {@const tool = tools.find(item => item.id === voiceId)!}
          <button class="nav-item" title={tool.label} aria-label={tool.label} class:selected={workspace === tool.id} aria-current={workspace === tool.id ? 'page' : undefined} class:transfer-target={transferTarget===tool.id} ondragover={event=>recordingOver(event,tool.id)} ondragleave={()=>transferTarget=''} ondrop={event=>recordingDrop(event,tool.id)} onclick={() => workspace = tool.id}>
            <svg viewBox="0 0 24 24" aria-hidden="true"><path d={tool.path}/></svg><span>{tool.label}</span><small class="tool-availability">{tool.id==='cleanup'||tool.id==='infilling'?'Experimental':'Preview'}</small>
          </button>
        {/each}
      </div>
      {#each tools.filter(tool => tool.id === 'transcription' || tool.id === 'dictation') as tool}
        <button class="nav-item" title={tool.label} aria-label={tool.label} class:selected={workspace === tool.id} aria-current={workspace === tool.id ? 'page' : undefined} class:transfer-target={transferTarget===tool.id} ondragover={event=>recordingOver(event,tool.id)} ondragleave={()=>transferTarget=''} ondrop={event=>recordingDrop(event,tool.id)} onclick={() => workspace = tool.id}>
          <svg viewBox="0 0 24 24" aria-hidden="true"><path d={tool.path}/></svg><span>{tool.label}</span><span class="nav-chevron" aria-hidden="true">›</span>
        </button>
      {/each}
      <div class="nav-divider"></div>
      <button class="nav-item" title="Projects" aria-label="Projects" class:selected={workspace === 'projects'} aria-current={workspace === 'projects' ? 'page' : undefined} onclick={() => workspace = 'projects'}><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M3 6h7l2 2h9v12H3V6Z"/></svg><span>Projects</span><small class="tool-availability">Upcoming</small></button>
    </nav>
    <div class="studio-footer">
    <SidebarUpdates onDetails={openUpdateSettings}/>
    <div class="studio-status-bar">
      <button class="studio-settings" aria-label="Settings" title="Settings" class:selected={workspace==='setup'} onclick={()=>{deviceStatusOpen=false;workspace='setup';}}><svg viewBox="0 0 24 24" aria-hidden="true"><path d="m9 3-.6 2.1-2 .9-2-.5-2 3.4 1.5 1.6v2.3l-1.5 1.6 2 3.4 2-.5 2 .9L9 21h4l.6-2.1 2-.9 2 .5 2-3.4-1.5-1.6v-2.3l1.5-1.6-2-3.4-2 .5-2-.9L13 3H9Z"/><circle cx="11" cy="12" r="3"/></svg></button>
      <button class="device-status" aria-label="Local device status" title={asrStatus?.state==='ready'?'Local device · Amharic transcription ready':'Local device · Check voice tools'} aria-expanded={deviceStatusOpen} aria-controls="local-device-status" onclick={()=>deviceStatusOpen=!deviceStatusOpen}><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M6 6h12v12H6V6Zm3 3h6v6H9V9ZM9 2v4m6-4v4M9 18v4m6-4v4M2 9h4m-4 6h4m12-6h4m-4 6h4"/></svg><span class="device-status-label">Local device</span><span class="device-ready-dot" class:ready={asrStatus?.state==='ready'} aria-hidden="true"></span><span class="device-status-chevron" aria-hidden="true">{deviceStatusOpen?'⌃':'⌄'}</span></button>
      {#if deviceStatusOpen}<section class="device-status-panel" id="local-device-status" aria-label="Local device details"><div class="device-panel-heading"><strong>On this computer</strong><button aria-label="Close device status" onclick={()=>deviceStatusOpen=false}>×</button></div><dl><div><dt>Processing</dt><dd>On this computer</dd></div><div><dt>Amharic transcription</dt><dd>{asrStatus?.state==='ready'?'Ready':asrStatus===null?'Checking…':'Needs setup'}</dd></div>{#if asrStatus?.modelVersion}<div><dt>ASR model</dt><dd>{asrStatus.modelVersion}</dd></div>{/if}{#if summary?.totalMemoryBytes}<div><dt>Memory</dt><dd>{(summary.totalMemoryBytes/1024**3).toFixed(1)} GB</dd></div>{/if}</dl><button class="device-tools-link" onclick={()=>{deviceStatusOpen=false;workspace='setup';}}>Voice tools &amp; downloads</button></section>{/if}
    </div>
    </div>
  </aside>
  <main id="welcome" class:transcription-page={workspace === 'infilling' || workspace === 'dictation' || workspace === 'cleanup' || workspace === 'home' || workspace === 'transcription' || workspace === 'tts'}>
    <header class="workspace-header"><span class="badge">{workspace === 'home' ? 'Home' : workspace === 'setup' ? 'Settings' : current?.label ?? 'VOICE STUDIO'}</span><div style="display:flex;align-items:center;gap:12px">{#if workspace==='dictation'}<button class="icon-button secondary" aria-label="Recording settings" aria-expanded={dictationSettingsOpen} aria-controls="dictation-recording-settings" onclick={()=>dictationSettingsOpen=!dictationSettingsOpen}><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" aria-hidden="true"><path d="m9 3-.6 2.1-2 .9-2-.5-2 3.4 1.5 1.6v2.3l-1.5 1.6 2 3.4 2-.5 2 .9L9 21h4l.6-2.1 2-.9 2 .5 2-3.4-1.5-1.6v-2.3l1.5-1.6-2-3.4-2 .5-2-.9L13 3H9Z"/><circle cx="11" cy="12" r="3"/></svg></button>{/if}<button class="language secondary" onclick={() => amharic = !amharic} aria-label="Switch interface language">{amharic ? 'English' : 'አማርኛ'}</button></div></header>
    {#if workspace!=='home'}<UpdateNotice onSettings={openUpdateSettings}/>{/if}
    {#if workspace !== 'infilling' && workspace !== 'dictation' && workspace !== 'cleanup' && workspace !== 'home' && workspace !== 'transcription' && workspace !== 'tts'}
    <section class="intro"><span class="eyebrow">AMHARIC · LOCAL WORKSPACE</span><h1>{current?.label ?? (workspace === 'projects' ? 'Your projects' : 'Settings')}</h1><p>{current?.description ?? (workspace === 'projects' ? 'Your saved work will live here, ready to pick up again.' : 'Prepare this computer for your voice tools.')}</p></section>
    {/if}
    {#if !desktop && workspace !== 'dictation' && workspace !== 'cleanup' && workspace !== 'home' && workspace !== 'transcription'}<div class="notice" role="status">Browser preview: open the desktop app to prepare your computer and use local voice tools.</div>{/if}
    {#if transferError}<p class="notice" role="alert">{transferError}</p>{/if}
    <div class="workspace-content">
      {#if workspace === 'home'}
      <Home onNavigate={target => {startupNavigationCancelled=true;if(target==='setup')void openDownloadSetup();else workspace=target;}} setupNotice={desktop?setupNotice:null} onSetup={openDownloadSetup} onUpdates={openUpdateSettings}/>
      {:else if workspace === 'tts'}
      <VoiceWorkspace bind:text bind:style={voiceStyle} bind:reference={referenceVoice} bind:referenceMedia bind:referenceText={referenceTranscript} onSetup={openDownloadSetup}/>
      {:else if workspace === 'cleanup'}
      <VoiceCleanup bind:recording={cleanupRecording} bind:jobId={cleanupJobId}/>
      {:else if workspace === 'infilling'}
      <VoiceEditing onSetup={openDownloadSetup}/>
      {:else if workspace === 'transcription'}
      <Transcription bind:historyOpen onSetup={openDownloadSetup}/>
      {:else if workspace === 'dictation'}
      <Dictation bind:historyOpen bind:settingsOpen={dictationSettingsOpen} onSetup={openDownloadSetup}/>
      {:else if workspace === 'projects'}
      <section class="card empty-state"><div class="empty-symbol" aria-hidden="true">▱</div><h2>Your voice projects</h2><p>Saved voice editing projects and their edit history are available in Voice Editing.</p><button onclick={() => workspace = 'infilling'}>Open voice editing projects</button></section>
      {:else if workspace !== 'setup'}
      <section class="card empty-state"><div class="empty-symbol" aria-hidden="true">≋</div><h2>{current?.label}</h2><p>Audio selection and replacement speech will be added in a later increment.</p><span class="pill">Workspace preview · Processing unavailable</span><button class="secondary" onclick={() => workspace = 'setup'}>View setup</button></section>
      {:else}
      <ResourceSetup installed={!setupNotice.visible} prominent={setupNotice.visible} onReady={()=>void checkAsr()}/>
      <HardwareSetup/>
      <GeneralSettings/>
      <UpdateSettings/>
      <ModelSettings/>
      <ModelLicenses/>
      <section class="card computer"><div class="section-heading"><span class="step">02</span><div><h2>{amharic ? 'የእርስዎ ኮምፒውተር' : 'A quick look inside'}</h2><p>Your computer</p></div></div><dl><div><dt>System</dt><dd>{summary?.os ?? 'Not detected yet'}</dd></div><div><dt>Processor</dt><dd>{summary?.arch ?? 'Unknown'}</dd></div><div><dt>CPU threads</dt><dd>{summary?.logicalCpus ?? 'Unknown'}</dd></div><div><dt>Memory</dt><dd>{summary?.totalMemoryBytes ? `${(summary.totalMemoryBytes / 1024 ** 3).toFixed(1)} GB` : 'Not measured'}</dd></div></dl><p class="hint">Voice setup supports Apple Silicon Macs with macOS 14 or later and 64-bit Windows 10 or later. Recognition uses compatible local CPU or GPU tools. Studio attempts model loading without a fixed RAM cutoff; performance depends on the available memory.</p></section>
      {/if}
    </div>{#if workspace !== 'infilling' && workspace !== 'dictation' && workspace !== 'cleanup' && workspace !== 'home' && workspace !== 'transcription' && workspace !== 'tts'}<footer><span>Amharic voice tools, on your computer.</span><span>v0 preview · Upcoming voice workspace drafts are not saved</span></footer>{/if}
  </main>

</div>

<style>
  .studio-footer{flex:none;width:100%}
  .nav-item.transfer-target{outline:2px solid var(--studio-accent);outline-offset:-2px;background:var(--studio-tone-12392e)}
  .tool-availability{margin-left:auto;font-size:9px;color:var(--studio-tone-a4b1c7);border:1px solid var(--studio-tone-33415a);border-radius:8px;padding:2px 5px;white-space:nowrap}.menu-collapsed .tool-availability{display:none}
</style>
