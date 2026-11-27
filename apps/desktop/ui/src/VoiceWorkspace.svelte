<script lang="ts">
  import {reviewDownload} from './download-review.js';
  import { onMount, tick } from 'svelte';
  import {invoke} from '@tauri-apps/api/core';
  import SpeechTextCheck from './SpeechTextCheck.svelte';
  import {prepareSpeechText} from './speech-text.js';
  import SpeechControls from './SpeechControls.svelte';
  import {speechCapabilities,defaultSpeechControls,speechControlError,speechGenerationRequest,savedSpeechSettings} from './speech-controls.js';
  import WaveformPlayer from './WaveformPlayer.svelte';
  import AudioTrim from './AudioTrim.svelte';
  import {croppedRecordingContext} from './audio-trim.js';
  import {ensurePreviewVoices} from './preview-voices.js';
  import { createProfile, listProfiles, saveProfile, deleteProfiles, renameProfile, setProfileImage } from './voice-profiles.js';
  import {recordingTitles,displayRecordingTitle,forgetRecordingTitles,refreshRecordingTitles} from './recording-titles.js';
  import HistoryPane from './HistoryPane.svelte';
  import HistoryEntry from './HistoryEntry.svelte';
  import VoiceProfileAvatar from './VoiceProfileAvatar.svelte';
  import WorkspaceCard from './WorkspaceCard.svelte';
  import {scrollToWorkspaceCard,workspaceCardId} from './card-navigation.js';
  import HistorySelection from './HistorySelection.svelte';
  import RecordingPanel from './RecordingPanel.svelte';
  import {recordingSelectionKey,toggleRecordingSelection} from './recording-selection.js';
  import type {HistoryDeleteItem,HistoryDeleteResult} from './history-selection-types';
  import { referenceError } from './voice-input.js';
  import {generationBlocker} from './voice-generation.js';
  import RecordingInput from './RecordingInput.svelte';
  import {readRecordingDrop,requestRecordingTransfer} from './recording-transfer.js';
  import RecordingActions from './RecordingActions.svelte';
  import {recordingFile,prepareRecordingFile,resolveRecording} from './recording-transfer.js';
  let { text = $bindable(''), style = $bindable('preset:default'), reference = $bindable<File | null>(null), referenceMedia=$bindable<import('./recording-transfer.js').RecordingContext|null>(null),referenceText=$bindable(''), onSetup }: {
    text: string; style: string; reference: File | null; referenceMedia?:import('./recording-transfer.js').RecordingContext|null;referenceText?:string;onSetup: () => void;
  } = $props();
  let profiles = $state<import('./voice-profiles.js').VoiceProfile[]>([]);
  let profileName = $state('');
  let saving = $state(false);
  const desktop='__TAURI_INTERNALS__' in window;
  function takeTitle(item:{id:string;name:string}){return displayRecordingTitle($recordingTitles,'voice-project',item.id,item.name);}
  async function renameVoice(profile:import('./voice-profiles.js').VoiceProfile,title:string){const updated=await renameProfile(profile.id,title,profile.name);if(mounted)profiles=profiles.map(item=>item.id===updated.id?updated:item);}
  async function changeProfileImage(id:string,image:string){const updated=await setProfileImage(id,image);if(mounted)profiles=profiles.map(item=>item.id===id?updated:item);}
  type Generation={id:string;name:string;text:string;revision:number;created:number;settings?:unknown;versionCount?:number;currentVersion?:string};
  type SpeechVersion={projectId:string;text:string;originalText?:string;settings?:unknown;created?:number;baseVersion?:string;reference?:{voice:{kind:string;label:string;id?:string|null};name:string;text:string;sha256?:string};result?:{model?:string;model_version?:string;revision?:string}};
  let generations=$state<Generation[]>([]),historySelected=$state<string[]>([]),profileSelected=$state<string[]>([]),historyLoading=$state(false),historyError=$state(''),profilesLoading=$state(false),profileError=$state(''),deleteBusy=$state(false),opening=$state(false);
  type GenerationStatus={controls?:import('./speech-controls.js').SpeechCapabilities;ready:boolean;consent:boolean;busy:boolean;stage:string;loaded:boolean;canPrepare:boolean;runtimePresent:boolean;provider:string|null;modelVersion:string|null;requiresReferenceText:boolean;defaultVoice:boolean;reason:string};
  let preparing=$state(false),generationError=$state('');
  let voiceTools=$state<GenerationStatus|null>(null),generating=$state(false),generatedUrl=$state(''),synthesisProject=$state('');
  let versions=$state<SpeechVersion[]>([]),versionId=$state('');
  const selectedVersion=$derived(versions.find(item=>item.projectId===versionId));
  let regeneration=$state<{groupId:string;expectedVersion:string;baseVersion:string;name:string}|null>(null);
  let draft:{text:string;style:string;reference:File|null;media:typeof referenceMedia;referenceText:string;controls:typeof speechControls}|null=null;
  let restoredVoice:{file:File;voice:{kind:string;label:string;id?:string|null}}|null=null;
  function restoreSettings(settings:unknown){const saved=savedSpeechSettings(settings),c=controlCapabilities;speechControls={...defaultSpeechControls(c),...(saved?{automaticSeed:false,seed:saved.seed,...(c.temperature&&saved.temperature!==undefined?{temperature:saved.temperature}:{}),...(c.sentencePause&&saved.sentencePause!==undefined?{sentencePause:saved.sentencePause}:{}),...(c.speed&&saved.speed!==undefined?{speed:saved.speed}:{}),...(c.duration&&saved.duration!==undefined?{useDuration:true,duration:saved.duration}:{}),...(c.numSteps&&saved.numSteps!==undefined?{numSteps:saved.numSteps}:{})}:{})};}
  async function selectVersion(id:string){if(locked)return;opening=true;inputError='';try{const version=versions.find(item=>item.projectId===id),item=generations.find(item=>item.id===synthesisProject);if(regeneration&&version&&item)await restoreRegeneration(item,version);await loadVersion(id);}catch(cause){inputError=String(cause);}finally{opening=false;}}
  async function loadVersion(id:string){const audio=await invoke<{bytes:number[]}>('get_voice_edit_audio',{projectId:id,kind:'synthesis'});if(!mounted)return;if(generatedUrl)URL.revokeObjectURL(generatedUrl);generatedUrl=URL.createObjectURL(new Blob([new Uint8Array(audio.bytes)],{type:'audio/wav'}));versionId=id;}
  async function restoreRegeneration(item:Generation,version:SpeechVersion){
    const source=await resolveRecording({kind:'editing',id:version.projectId,original:true});if(!mounted)return;
    if(!draft)draft={text,style,reference,media:referenceMedia,referenceText,controls:structuredClone($state.snapshot(speechControls))};
    stopReference();trimBackup=null;reference=recordingFile(source);referenceMedia=source.context;referenceText=version.reference?.text??source.context.text;style='reference';text=version.originalText??version.text;
    restoredVoice={file:reference,voice:version.reference?.voice??{kind:'reference',label:version.reference?.name??source.name}};
    generationCollapsed=false;restoreSettings(version.settings);regeneration={groupId:item.id,expectedVersion:item.currentVersion??versions.at(-1)?.projectId??item.id,baseVersion:version.projectId,name:takeTitle(item)};generationError='';await showScript();
  }
  async function regenerate(item:Generation){if(locked||!selectedVersion)return;opening=true;inputError='';try{await restoreRegeneration(item,selectedVersion);}catch(cause){inputError=`The saved reference could not be opened. ${String(cause)}`;}finally{opening=false;}}
  async function newSpeech(){if(locked)return;finishRegeneration();await showScript();}
  function finishRegeneration(){if(draft){referenceProfileId=draft.style.startsWith('profile:')?draft.style.slice(8):'';text=draft.text;style=draft.style;reference=draft.reference;referenceMedia=draft.media;referenceText=draft.referenceText;speechControls=draft.controls;}draft=null;regeneration=null;restoredVoice=null;generationError='';}
  function exportVersion(){if(!selectedVersion)return;const data={schemaVersion:1,cardId:synthesisProject,...$state.snapshot(selectedVersion)};const url=URL.createObjectURL(new Blob([JSON.stringify(data,null,2)],{type:'application/json'}));const link=document.createElement('a');link.href=url;link.download=`speech-${versionId}-metadata.json`;link.click();setTimeout(()=>URL.revokeObjectURL(url),1000);}
  const downloadPercent=$derived(preparing?Number(voiceTools?.stage.match(/([0-9]+)%/)?.[1])||undefined:undefined);
  const controlCapabilities=$derived(speechCapabilities(voiceTools));
  let speechControls=$state(defaultSpeechControls({}));
  const controlsError=$derived(speechControlError(speechControls,controlCapabilities));
  function resetSpeechControls(){speechControls=defaultSpeechControls(controlCapabilities);}
  let trimBusy=$state(false);
  let trimBackup=$state<{file:File;media:import('./recording-transfer.js').RecordingContext|null;text:string;style:string}|null>(null);
  const voiceTaskActive=$derived(generating||preparing||!!voiceTools?.busy);
  const locked=$derived(generating||preparing||saving||deleteBusy||opening||trimBusy||!!voiceTools?.busy);
  const historyEntries=$derived(generations.map(item=>({kind:'voice-project',id:item.id,name:takeTitle(item),revision:item.revision})));
  const profileEntries=$derived(profiles.map(profile=>({kind:'voice-profile',id:profile.id,name:profile.name})));
  let profileCollapsed=$state(false),generationCollapsed=$state(false);
  let mounted=false;
  async function openProfile(id:string,jump=true){if(locked)return;if(selectedProfile?.id!==id){stopReference();trimBackup=null;}style=`profile:${id}`;profileCollapsed=false;voicesOpen=true;await tick();if(mounted&&jump)scrollToWorkspaceCard(workspaceCardId('voice-profile',id));}
  function toggleProfile(id:string){if(locked)return;if(selectedProfile?.id===id&&!profileCollapsed){stopReference();profileCollapsed=true;}else void openProfile(id,false);}
  function toggleGeneration(item:Generation){if(locked)return;if(synthesisProject===item.id&&!generationCollapsed)generationCollapsed=true;else void openGeneration(item,false);}
  let referencePlayer=$state<AudioTrim>();
  function stopReference(){referencePlayer?.stop();}
  async function useTrimmed(value:{file:File;original:File;start:number;end:number;bytes:Uint8Array}){
    const previous={file:value.original,media:referenceContext,text:referenceText,style},name=selectedProfile?.name??'';
    const context=await croppedRecordingContext(referenceContext,value.start,value.end,value.bytes);
    if(!mounted||activeReference!==value.original)return;
    stopReference();trimBackup=previous;reference=value.file;referenceMedia=context;referenceText=context?.text??'';style='reference';profileCollapsed=false;profileName=name?`${name} (trimmed)`.slice(0,80):profileName;voicesOpen=true;inputError='';
    profileNotice=voiceTools?.requiresReferenceText===false?'Trimmed copy ready. Save it as a new reference profile.':context?.timingsCurrent?'Trimmed copy ready. Review its words and save it as a new reference profile.':'Trimmed copy ready. Enter the exact words in this segment before generating speech.';
    await tick();if(mounted)scrollToWorkspaceCard('voice-reference-editor');
  }
  async function undoTrim(){if(!trimBackup||locked)return;stopReference();const previous=trimBackup,saved=profiles.find(p=>`profile:${p.id}`===previous.style);reference=previous.file;referenceMedia=previous.media;referenceText=previous.text;style=saved&&(saved.referenceText??saved.sourceMedia?.text??'')===previous.text?previous.style:'reference';profileCollapsed=false;trimBackup=null;inputError='';profileNotice='Previous reference restored.';voicesOpen=true;await tick();if(mounted)scrollToWorkspaceCard('voice-reference-editor');}

  async function refreshTools(){if(desktop)try{const value=await invoke<typeof voiceTools>('get_voice_generation_status');if(mounted){const changed=voiceTools?.provider!==value?.provider;voiceTools=value;if(changed&&!regeneration)speechControls=defaultSpeechControls(speechCapabilities(value));}}catch{if(mounted)inputError='Speech tools could not be checked. Choose Check speech tools to retry.';}}
  async function refreshBusy(){if(!desktop)return;try{const wasBusy=!!voiceTools?.busy;if(generating||preparing||wasBusy){const value=await invoke<{busy:boolean;stage:string;loaded:boolean}>('get_voice_generation_activity');if(mounted&&voiceTools){voiceTools.busy=value.busy;voiceTools.stage=value.stage;voiceTools.loaded=value.loaded;if(wasBusy&&!value.busy){await refreshTools();await refreshHistory();}}}else{const value=await invoke<boolean>('get_voice_edit_busy');if(mounted&&voiceTools)voiceTools.busy=value;}}catch{/* Native processing still enforces the shared queue. */}}

  async function refreshProfiles(){if(profilesLoading)return;profilesLoading=true;profileError='';try{try{await ensurePreviewVoices();}catch{if(mounted)profileError='Preview voices could not be loaded. Your saved voices and the default voice are still available. Try Refresh voices again.';}const value=await listProfiles();if(mounted)profiles=value;}catch{if(mounted)profileError='Reference voices could not be loaded. Your saved profiles are unchanged. Try Refresh voices again.';}finally{if(mounted)profilesLoading=false;}}
  async function refreshHistory(){void refreshRecordingTitles();if(historyLoading||!desktop)return;historyLoading=true;historyError='';try{const value=await invoke<Generation[]>('list_voice_generations');if(mounted)generations=value;}catch{if(mounted)historyError='Generated speech could not be loaded. Your saved recordings are unchanged. Try Refresh again.';}finally{if(mounted)historyLoading=false;}}

  onMount(()=>{mounted=true;void refreshTools();void refreshHistory();void refreshProfiles();const timer=setInterval(()=>void refreshBusy(),1500);return()=>{mounted=false;clearInterval(timer);stopReference();if(generatedUrl)URL.revokeObjectURL(generatedUrl);};});
  async function grant(){try{await invoke('accept_voice_edit_consent');await refreshTools();}catch{inputError='Your permission choice could not be saved. Please try again.';}}
  async function prepare(){if(locked)return;const approvedPlan=await reviewDownload('speech');if(!approvedPlan)return;preparing=true;generationError='';if(voiceTools)voiceTools.stage='Preparing speech tools…';try{await invoke('prepare_voice_generation',{approvedPlan});}catch(cause){if(mounted)generationError=String(cause)==='paused'?'Setup paused. Your downloads are saved.':String(cause);}finally{if(mounted){preparing=false;await refreshTools();}}}
  async function cancel(){try{await invoke('cancel_voice_edit');}catch{inputError='The voice task could not be stopped yet. Please retry.';}}
  async function generate(){
    if(generationIssue)return;
    generating=true;generationError='';if(voiceTools)voiceTools.stage='Starting speech tools…';
    try{
      const generation=speechGenerationRequest(speechControls,controlCapabilities);
      const referenceInput=activeReference?{name:activeReference.name,bytes:Array.from(new Uint8Array(await activeReference.arrayBuffer()))}:await invoke<{name:string;bytes:number[]}>('get_default_voice');
      const voice=activeReference?(restoredVoice?.file===activeReference?restoredVoice.voice:{kind:selectedProfile?'profile':'reference',label:selectedProfile?.name??activeReference.name,id:selectedProfile?.id??null}):{kind:'default',label:'Default voice'};
      const result=await invoke<{groupId:string;projectId:string}>('synthesize_speech',{input:{...referenceInput,text:preparedText.text,originalText:text,...generation,referenceText:activeReference?referenceText:'',words:activeReference&&referenceContext?.timingsCurrent?referenceContext.words:[],sourceMedia:activeReference?$state.snapshot(referenceContext):null,voice:{...voice},groupId:regeneration?.groupId??null,expectedVersion:regeneration?.expectedVersion??null,baseVersion:regeneration?.baseVersion??null}});
      if(!mounted)return;
      generationCollapsed=false;synthesisProject=result.groupId;versions=await invoke<SpeechVersion[]>('get_speech_versions',{groupId:result.groupId});await loadVersion(result.projectId);await refreshHistory();
      if(regeneration)regeneration={...regeneration,expectedVersion:result.projectId,baseVersion:result.projectId};
      await tick();if(mounted)scrollToWorkspaceCard(workspaceCardId('voice-project',result.groupId));
    }catch(e){if(mounted)generationError=String(e);}finally{if(mounted){generating=false;await refreshTools();}}
  }
  let profileNotice = $state('');
  let voicesOpen = $state(false);
  let scriptInput=$state<HTMLTextAreaElement>();
  $effect(()=>{if(style==='reference')voicesOpen=true;});
  async function showScript(){stopReference();voicesOpen=false;await tick();if(mounted){scrollToWorkspaceCard('speech-composer');scriptInput?.focus({preventScroll:true});}}
  async function manageVoices(){voicesOpen=true;await tick();if(mounted)scrollToWorkspaceCard('voice-library');}
  function selectVoice(event:Event){
    const select=event.currentTarget as HTMLSelectElement,value=select.value;
    if(value==='import'){select.value=style;fileInput?.click();return;}
    stopReference();trimBackup=null;profileCollapsed=false;profileNotice='';style=value;
    if(style==='reference')referenceText=referenceMedia?.text??'';
  }
  const selectedProfile = $derived(profiles.find(profile => `profile:${profile.id}` === style));
  const activeReference = $derived(style === 'reference' ? reference : selectedProfile?.reference ?? null);
  const activeMedia=$derived(style==='reference'?referenceMedia:selectedProfile?.sourceMedia??null);
  const preparedText=$derived(prepareSpeechText(text));
  const generationIssue=$derived(generationBlocker(voiceTools,{style,hasReference:!!activeReference,referenceText,text,busy:locked})||(preparedText.issues.length?{code:'text-check',message:preparedText.issues[0]}:null)||(controlsError?{code:'settings',message:controlsError}:null));
  const referenceContext=$derived(activeMedia&&activeMedia.text!==referenceText?{...activeMedia,text:referenceText,timingsCurrent:false}:activeMedia);
  async function storeReference() {
    if (!reference || locked || profilesLoading) return;
    saving = true; inputError = ''; profileNotice = '';
    try {
      const profile = createProfile(profileName, reference,{referenceText,sourceMedia:referenceContext});
      await saveProfile(profile);
      profiles = [...profiles, profile];
      style = `profile:${profile.id}`;profileCollapsed=false;voicesOpen=true;
      profileName = '';
      profileNotice = voiceTools?.requiresReferenceText===false?'Reference profile saved on this computer. Choose it to generate speech.':'Reference profile saved on this computer. Review its exact words before generating speech.';
      await tick();if(mounted)scrollToWorkspaceCard(workspaceCardId('voice-profile',profile.id));
    } catch (error) { inputError = error instanceof Error ? error.message : 'The voice could not be saved.'; }
    finally { saving = false; }
  }
  async function openGeneration(item:Generation,jump=true){if(locked)return;if(regeneration&&regeneration.groupId!==item.id)finishRegeneration();generationCollapsed=false;if(synthesisProject!==item.id||!generatedUrl){opening=true;inputError='';try{const saved=await invoke<SpeechVersion[]>('get_speech_versions',{groupId:item.id});if(!mounted)return;await loadVersion(saved.at(-1)?.projectId??item.id);versions=saved;synthesisProject=item.id;}catch{if(mounted)inputError='This generated recording could not be opened. Select it again to retry.';}finally{if(mounted)opening=false;}}await tick();if(mounted&&jump)scrollToWorkspaceCard(workspaceCardId('voice-project',item.id));}
  async function deleteHistory(items:HistoryDeleteItem[]):Promise<HistoryDeleteResult>{
    const native=items.filter(item=>item.kind==='voice-project'),profileIds=items.filter(item=>item.kind==='voice-profile').map(item=>item.id);
    const result:HistoryDeleteResult=native.length?await invoke('delete_voice_history',{items:native}):{deleted:[],failed:[]};
    if(profileIds.length)try{await deleteProfiles(profileIds);result.deleted.push(...profileIds.map(id=>({kind:'voice-profile',id})));}catch{result.failed.push(...profileIds.map(id=>({kind:'voice-profile',id,message:'Saved profiles could not be removed. Retry deletion.'})));}
    forgetRecordingTitles(result.deleted);
    if(!mounted)return result;
    const removed=(kind:string,id:string)=>result.deleted.some(item=>item.kind===kind&&item.id===id);
    if(draft?.style.startsWith('profile:')&&removed('voice-profile',draft.style.slice(8)))draft={...draft,style:'preset:default',reference:null,media:null,referenceText:''};
    if(trimBackup?.style.startsWith('profile:')&&removed('voice-profile',trimBackup.style.slice(8)))trimBackup=null;
    if(selectedProfile&&removed('voice-profile',selectedProfile.id)){trimBackup=null;stopReference();style='preset:default';reference=null;referenceMedia=null;referenceText='';}
    if(removed('voice-project',synthesisProject)){if(generatedUrl)URL.revokeObjectURL(generatedUrl);generatedUrl='';synthesisProject='';versions=[];versionId='';}if(regeneration&&removed('voice-project',regeneration.groupId))finishRegeneration();
    profiles=profiles.filter(item=>!removed('voice-profile',item.id));generations=generations.filter(item=>!removed('voice-project',item.id));return result;
  }
  let referenceProfileId='';
  $effect(()=>{const selected=selectedProfile;if(selected&&selected.id!==referenceProfileId)referenceText=selected.referenceText??selected.sourceMedia?.text??'';referenceProfileId=selected?.id??'';});
  async function receiveRecording(value:import('./recording-transfer.js').RecordingPackage){const file=recordingFile(value);const problem=referenceError(file);if(problem)throw Error(problem);if(locked)throw Error('Finish the current voice task first.');trimBackup=null;reference=file;referenceMedia=value.context;referenceText=value.context.text;style='reference';voicesOpen=true;profileNotice='Recording, transcript and source details copied into this reference.';inputError='';}
  let fileInput = $state<HTMLInputElement>();
  let referenceUrl = $state('');
  let inputError = $state('');
  $effect(() => {
    const url = activeReference ? URL.createObjectURL(activeReference) : '';
    referenceUrl = url;
    return () => { if (url) URL.revokeObjectURL(url); };
  });
  async function chooseReference(event: Event) {
    const input = event.currentTarget as HTMLInputElement;
    const file = input.files?.[0];
    if (!file || locked) return;
    inputError = referenceError(file);
    if(inputError){input.value='';return;}
    opening=true;
    try{
      const prepared=await prepareRecordingFile(file);
      if(!mounted)return;
      const problem=referenceError(prepared);if(problem)throw Error(problem);
      stopReference();trimBackup=null;reference=prepared;referenceMedia=null;referenceText='';style='reference';voicesOpen=true;profileNotice='';
    }catch(cause){if(mounted)inputError=cause instanceof Error?cause.message:String(cause);}
    finally{if(mounted)opening=false;input.value='';}
  }
  function dropRecording(event:DragEvent){if(event.target instanceof Element&&event.target.closest('textarea,input')&&!Array.from(event.dataTransfer?.types??[]).some(type=>type==='application/x-ethiopic-studio-recording'||type==='Files'))return;event.preventDefault();event.stopPropagation();if(locked)return;try{requestRecordingTransfer('tts',readRecordingDrop(event.dataTransfer));}catch(cause){inputError=cause instanceof Error?cause.message:'Drop a recording card or audio file.';}}
</script>

<section class="voice-tool-layout" aria-label="Text to Speech editor" ondragover={event=>event.preventDefault()} ondrop={dropRecording}>
  <HistoryPane id="voice-history" label="Voice history" storageKey="ethiopic.voice">
    {#snippet controls()}
      <div class="voice-history-tools"><span>{historyEntries.length} generated</span><button class="secondary" aria-label="Refresh voice history" disabled={locked||historyLoading} onclick={refreshHistory}>{historyLoading?'Loading…':'Refresh'}</button></div>
      <HistorySelection entries={historyEntries} description="This removes the selected generated speech cards, their audio versions and related metadata. Saved reference voices, original files and exported copies stay unchanged." bind:selected={historySelected} bind:busy={deleteBusy} disabled={generating||preparing||saving||opening||trimBusy||!!voiceTools?.busy} onDelete={deleteHistory}/>
      {#if historyError}<p class="voice-history-error" role="alert">{historyError}</p>{/if}
    {/snippet}
    {#snippet history()}
      {#if historyLoading&&!historyEntries.length}<p class="voice-history-empty" role="status">Loading recordings…</p>{:else if !historyEntries.length}<p class="voice-history-empty">No generated speech yet.</p>{/if}
      <div class="voice-history-list">

        {#each generations as item (item.id)}<HistoryEntry name={takeTitle(item)} detail="Generated speech" selected={synthesisProject===item.id} disabled={locked} checked={historySelected.includes(recordingSelectionKey('voice-project',item.id))} onChecked={checked=>historySelected=toggleRecordingSelection(historySelected,'voice-project',item.id,checked)} selectionLabel={`Select generated speech: ${takeTitle(item)}`} aria-label={`Open generated speech: ${takeTitle(item)}`} data-generation-id={item.id} onSelect={()=>openGeneration(item)}/>{/each}
      </div>
    {/snippet}
    <div class="voice-workspace task-panel" data-card-scroller>
      <input class="visually-hidden" type="file" accept=".wav,.mp3,.flac,.ogg,.m4a" bind:this={fileInput} disabled={locked} onchange={chooseReference} aria-label="Choose reference audio"/>
      <section class="reference-panel" aria-label="Reference voices">
      <details class="reference-library" id="voice-library" bind:open={voicesOpen} ontoggle={()=>{if(!voicesOpen)stopReference();}}>
        <summary><span class="library-heading"><span class="library-icon" aria-hidden="true"><svg viewBox="0 0 24 24"><path d="M4 9v6m4-10v14m4-16v18m4-14v10m4-8v6"/></svg></span><span><strong>Reference voices</strong><small>Import, trim and save a voice for your scripts.</small></span></span><span class="library-count">{profiles.length} saved <span aria-hidden="true">{voicesOpen?'−':'+'}</span></span></summary>
        <div class="library-content">
          <div class="library-toolbar"><p>{activeReference?`Selected: ${selectedProfile?.name??activeReference.name}`:'Use the default voice or add a recording of your own.'}</p><div class="library-actions"><button class="secondary" disabled={locked||profilesLoading} onclick={refreshProfiles}>{profilesLoading?'Loading…':'Refresh voices'}</button><button class="secondary" disabled={locked} onclick={showScript}>Back to script</button><button class="secondary" disabled={locked} onclick={()=>fileInput?.click()}>Import reference audio</button></div></div>
          {#if profileError}<p class="voice-history-error" role="alert">{profileError}</p>{/if}
          <HistorySelection entries={profileEntries} bind:selected={profileSelected} bind:busy={deleteBusy} label="Reference voice selection" selectAllLabel="Select all voice profiles" deleteLabel="Delete selected profiles" singular="voice profile" plural="voice profiles" description="This removes the selected saved reference voices and their app-owned audio and profile images. Generated speech versions, original files and exported copies stay unchanged." disabled={generating||preparing||saving||opening||trimBusy||!!voiceTools?.busy} onDelete={deleteHistory}/>
          <p class="hint">Preview voices are for noncommercial use. <a href="/reference-voices/preview-v2/LICENSES.txt" target="_blank" rel="noreferrer">Voice pack licenses</a>. Portraits are fictional.</p>
          <RecordingInput target="tts" onReceive={receiveRecording} disabled={locked}/>
          {#if !selectedProfile&&style==='reference'}{@render referenceBody()}{/if}
          {#if profileNotice}<p class="hint" role="status">{profileNotice}</p>{/if}
          <div class="voice-card-list">
            {#each profiles as profile (profile.id)}
              <WorkspaceCard id={workspaceCardId('voice-profile',profile.id)} name={profile.name} rename={{kind:'voice-profile',id:profile.id,maxLength:80,onSave:title=>renameVoice(profile,title)}} detail={profile.preset?'Noncommercial preview voice':'Saved voice profile'} expanded={selectedProfile?.id===profile.id&&!profileCollapsed} disabled={locked} checked={profileSelected.includes(recordingSelectionKey('voice-profile',profile.id))} onChecked={checked=>profileSelected=toggleRecordingSelection(profileSelected,'voice-profile',profile.id,checked)} onOpen={()=>toggleProfile(profile.id)}>
                {#snippet leading()}<VoiceProfileAvatar name={profile.name} image={profile.image} disabled={locked} onSave={image=>changeProfileImage(profile.id,image)}/>{/snippet}
                {#snippet summary()}<p class="card-transcript" lang="am">{profile.referenceText||profile.sourceMedia?.text||'Open this card to hear the reference voice.'}</p>{/snippet}
                {@render referenceBody()}
              </WorkspaceCard>
            {:else}<p class="library-empty">{profilesLoading?'Loading reference voices…':'No saved reference voices yet. The default voice is ready to use when speech tools are installed.'}</p>{/each}
          </div>
        </div>
      </details>
      </section>
      {#snippet speechComposer()}
      <section class="speech-composer" class:card-composer={!!regeneration} id="speech-composer" aria-label="Speech script">
        <header class="composer-heading"><div><h2>{regeneration?'Edit speech':'Synthesize speech'}</h2><p>{regeneration?'Adjust the script, voice or controls, then regenerate. Each version stays in this card.':'Write or paste text, then select a voice.'}</p></div><span class="language-tag">Amharic</span></header>
        <label class="script-label" for="voice-script">Script</label>
        <textarea class="voice-script" id="voice-script" bind:this={scriptInput} disabled={locked} maxlength="12000" lang="am" bind:value={text} spellcheck="false" oninput={()=>generationError=''} placeholder="የሚነበበውን ጽሑፍ እዚህ ይጻፉ…" aria-describedby="script-help"></textarea>
        <div class="voice-script-meta"><span id="script-help">Write or paste the text to turn into audio.</span><span>{Array.from(text).length.toLocaleString()} / 12,000</span></div>
        <SpeechTextCheck prepared={preparedText} disabled={locked} onApply={()=>text=preparedText.text}/>
        <div class="voice-controls">
          <div class="voice-style"><span class="voice-avatar" aria-hidden="true">≋</span><div><label for="voice-choice">Choose a voice</label><select id="voice-choice" value={style} disabled={locked} onchange={selectVoice}>
            <option value="preset:default">Default voice</option>
            {#if style.startsWith('profile:') && !selectedProfile}<option value={style} disabled>Loading saved voice…</option>{/if}
            <optgroup label="Saved reference voices">{#each profiles as profile}<option value={`profile:${profile.id}`}>{profile.name}{profile.preset?' · noncommercial preview':''}</option>{/each}</optgroup>
            {#if reference}<option value="reference">{reference.name} · Current reference</option>{/if}
            <option value="import">Import a new voice…</option>
          </select></div></div>
          <button class="secondary reference-import" disabled={locked} onclick={manageVoices} aria-expanded={voicesOpen} aria-controls="voice-library">Manage voices</button>
        </div>
        <SpeechControls bind:values={speechControls} capabilities={controlCapabilities} disabled={locked} error={controlsError} onReset={resetSpeechControls}/>
        {#if voiceTools&&!voiceTools.consent}<div class="voice-consent"><p>Only use reference recordings you have the right to use. Studio is licensed for non-commercial use; model licenses are separate.</p><button class="secondary" onclick={grant}>I have permission to use this recording</button></div>{/if}
        <div class="voice-action-bar">
          <div class="generation-status"><p id="generation-readiness" role="status">{voiceTaskActive?voiceTools?.stage||(preparing?'Preparing speech tools…':'Generating your speech…'):generationIssue?.message||'Ready to generate speech on this computer.'}</p>{#if voiceTaskActive}<progress aria-label={preparing?'Speech tools download progress':'Speech generation progress'} max="100" value={downloadPercent}></progress>{/if}</div>
          <div class="voice-generate"><button aria-describedby="generation-readiness" disabled={!!generationIssue} onclick={generate}>{generating?'Generating…':regeneration?'Regenerate speech':'Generate speech'}</button>
            {#if regeneration}<button class="secondary" disabled={locked} onclick={newSpeech}>Done editing</button>{/if}
            {#if generationIssue?.code==='setup'}<button class="secondary" disabled={locked} onclick={prepare}>Download speech tools</button>{/if}
            {#if voiceTaskActive}<button class="secondary" onclick={cancel}>Cancel voice task</button>{/if}
          </div>
        </div>
        {#if generationError}<p class="generation-error" role="alert">{generationError}</p>{/if}
        <div class="composer-footnote"><span>Script saved with generated audio.</span><details class="speech-tools"><summary>Speech tools</summary><div><button class="voice-setup-link" disabled={locked} onclick={refreshTools}>Check speech tools</button><button class="voice-setup-link" onclick={onSetup}>Settings &amp; model details</button></div></details></div>
      </section>
      {/snippet}
      {#if !regeneration}{@render speechComposer()}{:else}<div class="new-speech-toolbar"><span>Editing {regeneration.name}</span><button class="secondary" disabled={locked} onclick={newSpeech}>New speech</button></div>{/if}
      {#if inputError}<p class="notice" role="alert">{inputError}</p>{/if}
        {#snippet referenceBody()}<div class="reference-card-body" id="voice-reference-editor">
    <RecordingPanel title="Reference recording" label="Reference recording"><div class="reference-details">
      {#if activeReference}
        <div class="reference-file"><div><strong>{selectedProfile ? selectedProfile.name : activeReference.name}</strong><span>{(activeReference.size / 1024 / 1024).toFixed(1)} MB · {selectedProfile ? 'Saved reference voice' : 'Current reference recording'}</span></div><button class="secondary" disabled={locked} onclick={() => fileInput?.click()}>Replace recording</button><button class="secondary" disabled={locked} onclick={() => { trimBackup=null;reference = null;referenceMedia=null;referenceText='';style = 'preset:default'; inputError = ''; }}>Clear selection</button></div>
        <AudioTrim bind:this={referencePlayer} file={activeReference} src={referenceUrl} label="Reference voice" id="voice-reference" disabled={locked} bind:busy={trimBusy} onApply={useTrimmed}/>
        {#if trimBackup}<button class="secondary undo-trim" disabled={locked} onclick={undoTrim}>Undo trim</button>{/if}
      {:else}<button class="secondary reference-import" disabled={locked} onclick={() => fileInput?.click()}>Import reference voice</button><p>Choose a clear recording to prepare this voice.</p>{/if}
      {#if reference && style === 'reference'}<div class="save-reference"><label for="profile-name">Save a project voice profile</label><div><input id="profile-name" disabled={locked} maxlength="80" bind:value={profileName} placeholder="Give this voice a name"/><button class="secondary" disabled={locked || profilesLoading || !profileName.trim()} onclick={storeReference}>{saving ? 'Saving…' : 'Save reference profile'}</button></div></div>{/if}
      {#if selectedProfile}<label class="voice-card-select"><input type="checkbox" aria-label="Select current voice profile" disabled={locked} checked={profileSelected.includes(recordingSelectionKey('voice-profile',selectedProfile.id))} onchange={event=>{if(selectedProfile)profileSelected=toggleRecordingSelection(profileSelected,'voice-profile',selectedProfile.id,event.currentTarget.checked);}}/>Select profile</label>{/if}
      {#if referenceContext?.editedRegions.length}<p>Contains edited or generated speech.</p>{/if}
      <small>Stored locally. Profiles hold your reference recording; compatible voice tools generate speech from it.</small>
    </div></RecordingPanel>
  {#if activeReference}<RecordingActions source={{kind:'file',file:activeReference,context:referenceContext,text:referenceText}} label="reference recording" owner="tts" disabled={locked}/>{/if}
  {#if activeReference}<label for="voice-reference-text">{voiceTools?.requiresReferenceText===false?'Reference transcript (optional)':'Exact words in the reference WAV'}</label><textarea id="voice-reference-text" disabled={locked} lang="am" bind:value={referenceText} rows="2" placeholder="Transcribe the reference recording exactly."></textarea>{/if}
  <button class="secondary use-reference" disabled={locked||!activeReference} onclick={showScript}>Use this voice in script</button>
  </div>{/snippet}

      <section class="generated-library" aria-label="Generated audio">
        <header class="results-heading"><h2>Generated audio</h2><span>{generations.length} {generations.length===1?'recording':'recordings'}</span></header>
        <div class="voice-card-list">
          {#each generations as item (item.id)}
            {@const active=synthesisProject===item.id?selectedVersion:undefined}
            {@const settings=savedSpeechSettings(active?.settings??item.settings)}
            <WorkspaceCard id={workspaceCardId('voice-project',item.id)} name={takeTitle(item)} rename={{kind:'voice-project',id:item.id}} detail={`${item.versionCount??1} ${(item.versionCount??1)===1?'version':'versions'}`} expanded={synthesisProject===item.id&&!generationCollapsed} disabled={locked} checked={historySelected.includes(recordingSelectionKey('voice-project',item.id))} onChecked={checked=>historySelected=toggleRecordingSelection(historySelected,'voice-project',item.id,checked)} onOpen={()=>toggleGeneration(item)}>
              {#snippet summary()}<p class="card-transcript" lang="am">{item.text}</p>{/snippet}
              <div class="generated-card-body">
                <div class="version-toolbar"><label>Audio version <select aria-label="Audio version" value={versionId} disabled={locked} onchange={event=>selectVersion(event.currentTarget.value)}>{#each versions as version,index}<option value={version.projectId}>Version {index+1}{version.created?` · ${new Date(version.created*1000).toLocaleString()}`:''}</option>{/each}</select></label>{#if regeneration?.groupId!==item.id}<button class="secondary" disabled={locked||!active} onclick={()=>regenerate(item)}>Edit &amp; regenerate</button>{/if}</div>
                {#if regeneration?.groupId===item.id}{@render speechComposer()}{:else}<div class="generated-script"><p class="card-transcript" lang="am">{active?.text??item.text}</p></div>{/if}
                <details class="take-settings"><summary>Generation settings</summary>{#if settings}<dl>{#if settings.temperature!==undefined}<div><dt>Temperature</dt><dd>{settings.temperature}</dd></div>{/if}{#if settings.speed!==undefined}<div><dt>Speaking speed</dt><dd>{settings.speed}×</dd></div>{/if}{#if settings.duration!==undefined}<div><dt>Target duration</dt><dd>{settings.duration} seconds</dd></div>{/if}{#if settings.numSteps!==undefined}<div><dt>Generation steps</dt><dd>{settings.numSteps}</dd></div>{/if}{#if settings.sentencePause!==undefined}<div><dt>Sentence pause</dt><dd>{settings.sentencePause} seconds</dd></div>{/if}<div><dt>Variation seed</dt><dd>{settings.seed}</dd></div></dl>{:else}<p>Settings were not recorded for this older take.</p>{/if}{#if active?.reference}<p>Voice: {active.reference.voice.label}<br/>Reference: {active.reference.name}</p>{/if}{#if active?.result?.model}<p>Model: {active.result.model} {active.result.model_version??''}<br/>Revision: {active.result.revision??'Not recorded'}</p>{/if}<button class="secondary" disabled={!active||locked} onclick={exportVersion}>Export version metadata</button></details>
                {#if generatedUrl&&synthesisProject}<RecordingActions source={{kind:'synthesis',id:versionId}} label="generated speech" owner="tts" disabled={locked} copyPath/>{/if}
                {#if generatedUrl}<RecordingPanel title="Generated speech">{#snippet actions()}<label class="voice-card-select"><input type="checkbox" aria-label="Select current generated speech" disabled={locked} checked={historySelected.includes(recordingSelectionKey('voice-project',synthesisProject))} onchange={event=>historySelected=toggleRecordingSelection(historySelected,'voice-project',synthesisProject,event.currentTarget.checked)}/>Select recording</label>{/snippet}<WaveformPlayer src={generatedUrl} label="Generated speech" id="voice-synthesis"/></RecordingPanel>{/if}
              </div>
            </WorkspaceCard>
          {:else}<p class="results-empty">Your generated recordings will appear here with playback and export options.</p>{/each}
        </div>
      </section>
    </div>
  </HistoryPane>
</section>
<style>
  .reference-panel{flex:none;min-width:0}.reference-panel :global(.history-selection){padding:0}

  .new-speech-toolbar{display:flex;justify-content:space-between;align-items:center;gap:12px;font-size:12px;color:var(--studio-muted)}.new-speech-toolbar span{min-width:0;overflow-wrap:anywhere}.new-speech-toolbar button{flex:none;font-size:12px}.speech-composer.card-composer{padding:0;border:0;background:transparent}.card-composer .composer-heading h2{font-size:17px}

  .version-toolbar{display:flex;align-items:end;justify-content:space-between;gap:12px;flex-wrap:wrap}.version-toolbar label{display:flex;flex-direction:column;gap:7px;font-size:11px;color:var(--studio-muted)}.version-toolbar select{max-width:100%;padding:8px;border:1px solid var(--studio-border);border-radius:8px;background:var(--studio-page);color:var(--studio-text)}
  .take-settings{font-size:11px;color:var(--studio-muted)}.take-settings summary{cursor:pointer}.take-settings dl{margin:12px 0}.take-settings dl>div{font-size:11px;gap:12px}.take-settings button{font-size:10px;padding:7px 11px}.take-settings p{font-size:11px;line-height:1.7}

  .speech-composer{flex:none;min-width:0;padding:24px;background:var(--studio-panel);border:1px solid var(--studio-border);border-radius:16px;scroll-margin:12px}
  .composer-heading,.results-heading{display:flex;justify-content:space-between;align-items:start;gap:16px}.composer-heading{margin-bottom:22px}.composer-heading h2{font-size:21px;letter-spacing:-.4px;margin:0 0 7px}.composer-heading p{font-size:12px;line-height:1.6;color:var(--studio-muted);margin:0}.language-tag{border:1px solid var(--studio-border);padding:5px 9px;border-radius:20px;font-size:10px;color:var(--studio-muted);flex:none}
  .script-label{display:block;font-size:13px;font-weight:600;margin-bottom:10px;color:var(--studio-text)}.voice-script{display:block;width:100%;box-sizing:border-box;flex:none;height:clamp(200px,27vh,300px);min-height:180px;resize:vertical;border:1px solid var(--studio-border);border-radius:12px;background:var(--studio-page);color:var(--studio-text);padding:16px 18px;font-size:21px;line-height:1.9}.voice-script:focus{outline:2px solid var(--studio-focus);outline-offset:2px}.voice-script-meta{margin-top:10px;font-size:10px;line-height:1.6;flex-wrap:wrap}.voice-script-meta>span:last-child{white-space:nowrap}
  .voice-controls{padding:20px 0 16px;margin-top:6px;border-bottom:1px solid var(--studio-divider);gap:12px}.voice-style{flex:1;min-width:0}.voice-style>div{min-width:0;flex:1}.voice-style select{font-size:12px;max-width:100%;width:100%}.voice-avatar{width:34px;height:34px;font-size:21px}.reference-import{font-size:11px;padding:9px 12px}
  .voice-action-bar{padding:18px 0 8px;background:transparent;border:0;border-radius:0;gap:14px}.voice-action-bar p{margin:0;line-height:1.6;max-width:none;font-size:11px}.generation-status{flex:1;min-width:0}.voice-generate{align-items:stretch;flex:none}.voice-generate>button{min-height:42px;padding:11px 20px;font-size:12px}.voice-action-bar progress{width:100%;height:8px;accent-color:var(--studio-accent);margin-top:10px}.generation-error{color:var(--studio-tone-ffb2be);white-space:pre-wrap;overflow-wrap:anywhere;font-size:12px;line-height:1.7}.voice-consent{font-size:12px;margin-top:15px;line-height:1.7}.voice-consent button{font-size:11px}
  .composer-footnote{display:flex;justify-content:space-between;align-items:start;gap:16px;margin-top:10px;font-size:10px;color:var(--studio-muted)}.speech-tools{position:relative;text-align:right}.speech-tools summary{cursor:pointer}.speech-tools>div{display:flex;flex-direction:column;align-items:end;gap:8px;padding-top:10px}.voice-setup-link{background:transparent;color:var(--studio-muted);padding:0;font-size:10px;font-weight:400;text-decoration:underline}
  .reference-library{flex:none;min-width:0;border:1px solid color-mix(in srgb,var(--studio-accent) 35%,var(--studio-border));border-inline-start:3px solid var(--studio-accent);border-radius:14px;background:color-mix(in srgb,var(--studio-accent) 9%,var(--studio-panel));scroll-margin:12px}.library-heading{display:flex;align-items:center;gap:12px;min-width:0}.library-icon{display:grid;place-items:center;width:36px;height:36px;flex:none;border-radius:10px;background:color-mix(in srgb,var(--studio-accent) 14%,transparent);color:var(--studio-accent)}.library-icon svg{width:19px;height:19px}.reference-library>summary{cursor:pointer;list-style:none;display:flex;align-items:center;justify-content:space-between;gap:14px;padding:18px 20px}.reference-library>summary::-webkit-details-marker{display:none}.reference-library strong{display:block;font-size:13px;font-weight:600}.reference-library small{display:block;font-size:11px;line-height:1.6;color:var(--studio-muted);margin-top:5px}.library-count{display:flex;gap:14px;align-items:center;white-space:nowrap;font-size:10px;color:var(--studio-muted)}.library-count>span{font-size:19px;line-height:1}.library-content{display:flex;flex-direction:column;gap:18px;padding:0 20px 20px}.library-toolbar{display:flex;align-items:center;justify-content:space-between;gap:14px;flex-wrap:wrap}.library-toolbar p,.library-empty{font-size:12px;color:var(--studio-muted);line-height:1.7;margin:0;overflow-wrap:anywhere}.library-actions{display:flex;gap:8px;flex-wrap:wrap}.library-toolbar button{font-size:11px}.library-content :global(.recording-input){margin:0}.use-reference{align-self:flex-start;font-size:12px}
  .generated-library{min-width:0;flex:none;padding-top:8px}.results-heading{margin:0 0 16px;align-items:center}.results-heading h2{font-size:15px;font-weight:600;margin:0}.results-heading>span{font-size:10px;color:var(--studio-muted)}.results-empty{border:1px dashed var(--studio-border);border-radius:14px;padding:24px;font-size:12px;color:var(--studio-muted);line-height:1.8;margin:0}.generated-script{display:flex;align-items:start;gap:18px;justify-content:space-between}.generated-script .card-transcript{max-height:none;margin:0}
  .reference-details>small{display:block;margin-top:12px;font-size:10px;color:var(--studio-muted)}.undo-trim{margin-top:12px;font-size:11px}
  .voice-card-list,.reference-card-body,.generated-card-body{display:flex;flex-direction:column;gap:18px;min-width:0}.card-transcript{font-size:13px;line-height:1.8;color:var(--studio-muted);white-space:pre-wrap;overflow-wrap:anywhere;margin:14px 0 0;max-height:5.4em;overflow:hidden}.reference-card-body textarea{width:100%;box-sizing:border-box;resize:vertical;background:var(--studio-panel);color:var(--studio-text);border:1px solid var(--studio-border);border-radius:10px;padding:12px;font:inherit}
  .voice-tool-layout{display:flex;flex:1;min-width:0;min-height:0;overflow:hidden}.voice-history-tools{display:flex;justify-content:space-between;align-items:center;gap:10px;padding:0 18px 12px;font-size:11px;color:var(--studio-muted);flex:none}.voice-history-tools button{font-size:10px;padding:6px 9px}.voice-history-list{display:flex;flex-direction:column;gap:6px}.voice-history-empty{padding:8px;font-size:12px;line-height:1.7;color:var(--studio-muted)}.voice-history-error{margin:0 18px 12px;font-size:12px;line-height:1.7;color:var(--studio-tone-ffb2be)}.voice-card-select{display:flex;align-items:center;gap:7px;font-size:11px;color:var(--studio-muted)}.voice-card-select input{width:16px;height:16px;accent-color:var(--studio-accent)}
  @media(max-width:1000px){.speech-composer{padding:20px}.voice-action-bar{flex-direction:column;align-items:stretch}.voice-generate{display:flex;flex-direction:row;flex-wrap:wrap}.voice-generate>button{flex:1}.voice-controls{align-items:center}.voice-style select{max-width:100%}}
  @media(max-width:700px){.speech-composer{padding:16px}.composer-heading{gap:10px}.composer-heading h2{font-size:19px}.voice-script{font-size:19px;padding:13px;min-height:180px}.voice-controls{align-items:stretch}.voice-style{width:100%;flex-basis:100%}.reference-import{justify-content:center}.reference-library>summary{padding:16px}.library-content{padding:0 16px 16px}.generated-script{flex-direction:column}.composer-footnote{flex-wrap:wrap}}
</style>

