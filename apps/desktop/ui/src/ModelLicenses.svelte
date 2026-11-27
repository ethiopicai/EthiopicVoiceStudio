<script lang="ts">
  import decoderLicense from './symphonia-license.txt?raw';
  import noiseLicense from './noise-reduction-license.txt?raw';
  import {onMount} from 'svelte';
  import {invoke} from '@tauri-apps/api/core';
  type Model={id:string;release:string};
  let inventory=$state<Model[]>([]),aliases=$state<Record<string,string>>({});
  function release(id:string|null){if(!id)return 'Unavailable';const canonical=aliases[id]??id;return inventory.find(model=>model.id===canonical)?.release??canonical;}
  let editing=$state<{runtimePresent:boolean;ready:boolean;loaded:boolean;selection:{tier:string;tts:string|null;reason:string}}|null>(null);
  let generation=$state<{provider:string|null;modelVersion:string|null;runtimePresent:boolean;ready:boolean;loaded:boolean;reason:string}|null>(null);
  onMount(()=>{let alive=true;if('__TAURI_INTERNALS__' in window)void Promise.allSettled([
    invoke<typeof generation>('get_voice_generation_status'),invoke<typeof editing>('get_voice_edit_status'),invoke<{manifest:{variants:Model[];aliases:Record<string,string>}}>('get_studio_model_source')
  ]).then(([speech,editor,source])=>{if(!alive)return;if(source.status==='fulfilled'){inventory=source.value.manifest.variants;aliases=source.value.manifest.aliases;}if(speech.status==='fulfilled')generation=speech.value;if(editor.status==='fulfilled')editing=editor.value;});return()=>{alive=false;};});
</script>
<section class="card" aria-label="Models and licenses">
  <h2>Models &amp; licenses</h2>
  <details><summary>Text to Speech · Local voice models</summary>
    {#if generation}<dl><div><dt>Selected model</dt><dd>{release(generation.provider)}</dd></div><div><dt>Version</dt><dd>{generation.modelVersion??'See model release'}</dd></div><div><dt>Local runtime</dt><dd>{generation.runtimePresent?'Installed · verified before use':'Not installed'}</dd></div><div><dt>Model cache</dt><dd>{generation.ready?'Ready':'Download or setup needed'}</dd></div><div><dt>Speech engine</dt><dd>{generation.loaded?'Loaded · waiting for jobs':'Not loaded · starts when needed'}</dd></div></dl><p>{generation.reason}</p>{/if}
    <p>Speech is generated locally. The small CPU model supports the bundled default voice and reference recordings without a reference transcript. Its Amharic weights are for non-commercial use.</p>
  </details>
  <details><summary>Voice Editing · Local runtime and models</summary>
    {#if editing}<dl><div><dt>Hardware tier</dt><dd>{editing.selection.tier}</dd></div><div><dt>Selected voice model</dt><dd>{editing.selection.tts?release(editing.selection.tts):'Unavailable on this tier'}</dd></div><div><dt>Local runtime</dt><dd>{editing.runtimePresent?'Installed · verified before use':'Not installed for voice editing'}</dd></div><div><dt>Model cache</dt><dd>{editing.ready?'Ready':'Setup needed or unavailable on this hardware'}</dd></div><div><dt>Resident voice engine</dt><dd>{editing.loaded?'Loaded · shared by editing and synthesis':'Not loaded'}</dd></div></dl><p>{editing.selection.reason}</p>{/if}
    <p>The editor shares one voice model with synthesis. Voice tools are installed and managed by Studio; no separate Python setup is needed.</p>
  </details>
  <p>Only edit recordings you have the right to change. Synthetic regions remain marked in projects and export logs.</p>
  <h3>Model weights</h3>
  <dl aria-label="Model weight licenses">
    <div><dt>Amharic speech-generation model</dt><dd>CC BY 4.0 · EthiopicAI weights trained from scratch · commercial use with attribution</dd></div>
    <div><dt>Amharic ASR and word alignment</dt><dd>CC BY 4.0 · EthiopicAI weights · commercial use with attribution</dd></div>
    <div><dt>Amharic text corrector</dt><dd>CC BY 4.0 · EthiopicAI weights · commercial use with attribution</dd></div>
    <div><dt>Higgs Audio v2 codec</dt><dd>Boson Higgs Audio 2 Community License Agreement and incorporated Meta Llama 3 agreement</dd></div>
    <div><dt>Pocket TTS model family</dt><dd>Kyutai base weights CC BY 4.0 plus acceptable-use terms · separate Amharic fine-tune non-commercial</dd></div>
  </dl>
  <p>Attribution: EthiopicAI — Amharic speech and text models, with the model release and CC BY 4.0 license. Indicate modifications when sharing adapted weights.</p>
  <p>CC BY 4.0 license: https://creativecommons.org/licenses/by/4.0/</p>
  <details><summary>Upstream software &amp; attribution</summary>
    <dl aria-label="Software licenses">
      <div><dt>Speech-generation library · OmniVoice</dt><dd>Apache-2.0 · open-source software</dd></div>
      <div><dt>Upstream ASR · Meta Omnilingual ASR</dt><dd>Apache-2.0 · code and base models</dd></div>
      <div><dt>Voice editor code</dt><dd>Apache-2.0 · alignment routine adapted from MIT-licensed code</dd></div>
      <div><dt>Text-corrector inference code</dt><dd>MIT</dd></div>
      <div><dt>Pocket TTS software</dt><dd>MIT</dd></div>
    </dl>
    <p>EthiopicAI's speech-generation weights were trained from scratch. The upstream pretrained OmniVoice checkpoint and its weight license do not describe these weights.</p>
  </details>
  <p>Studio application: PolyForm Noncommercial. Model-weight licenses are separate. Bundled preview voice recordings retain their own noncommercial terms.</p>
  <p class="attribution">Built with Higgs Materials licensed from Boson AI USA, Inc., Copyright Boson AI USA, Inc., All Rights Reserved and Meta Llama 3 licensed under the Meta Llama 3 Community License, Copyright Meta Platforms, Inc., All Right Reserved.</p>
  <details><summary>Audio decoder · Software license</summary><p>Symphonia 0.5.5 · MPL-2.0 · Unmodified source: https://github.com/pdeljanov/Symphonia/tree/v0.5.5</p><pre>{decoderLicense}</pre></details>
  <details><summary>Amharic text processing · Software license</summary><p>Amharic Text Processor 0.1.3 · Ethiopic AI · MIT. Abbreviation rules compiled from AbbreviationExpander, revision 3175feaefb31e7af48118ac4760fa3cdc167c7fe. Source: https://github.com/ethiopicai/Amharic-Text-Processor</p></details>
  <details><summary>Recognition noise reduction · Software license</summary><p>Inworld Voice Isolator v1 · Copyright 2026 Inworld AI · code and weights Apache-2.0. Training data notices: VocalSet and IndicVoices-R (CC BY 4.0), VCTK (ODC-By 1.0). Full LICENSE and NOTICE accompany the optional installed package.</p><pre>{noiseLicense}</pre></details>
</section>
<style>pre{white-space:pre-wrap;overflow-wrap:anywhere;max-height:260px;overflow:auto;font-size:11px;line-height:1.6}h2{font-size:17px}h3{font-size:13px;margin-top:22px}summary{cursor:pointer;font-size:13px;padding:12px 0}p{font-size:12px;color:var(--studio-tone-a3b3ca);line-height:1.8}dl div{font-size:12px;gap:16px;align-items:start}dd{max-width:65%;overflow-wrap:anywhere}.attribution{font-size:10px}</style>
