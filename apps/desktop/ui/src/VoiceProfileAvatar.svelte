<script lang="ts">
  import {profileImageFromFile,DEFAULT_PROFILE_IMAGES} from './profile-image.js';
  let {name,image,size=48,disabled=false,onSave}:{name:string;image?:string;size?:number;disabled?:boolean;onSave?:(image:string)=>Promise<void>}=$props();
  let input=$state<HTMLInputElement>(),menu=$state<HTMLDetailsElement>(),busy=$state(false),error=$state('');
  const displayedImage=$derived(image||DEFAULT_PROFILE_IMAGES[0].src);
  async function usePreset(source:string){if(disabled||busy)return;busy=true;error='';try{await onSave?.(source);if(menu)menu.open=false;}catch(cause){error=cause instanceof Error?cause.message:'The profile image could not be saved. Try again.';}finally{busy=false;}}
  async function choose(event:Event){
    const target=event.currentTarget as HTMLInputElement,file=target.files?.[0];if(!file)return;
    busy=true;error='';
    try{await onSave?.(await profileImageFromFile(file));}catch(cause){error=cause instanceof Error?cause.message:'The profile image could not be saved. Try again.';}finally{busy=false;target.value='';}
  }
</script>
{#snippet picture()}
  <img src={displayedImage} alt=""/>
{/snippet}
{#if onSave}
  <div class="avatar-upload" style={`--avatar-size:${size}px`}>
    <button class="profile-avatar editable" type="button" aria-label={`Change profile image for ${name}`} title={busy?'Saving image…':'Change profile image'} disabled={disabled||busy} onclick={()=>input?.click()} aria-busy={busy}>
      {@render picture()}<span class="avatar-edit" aria-hidden="true">{busy?'…':'+'}</span>
    </button>
    <input class="visually-hidden" type="file" accept="image/jpeg,image/png,image/webp" aria-label={`Upload profile image for ${name}`} bind:this={input} disabled={disabled||busy} onchange={choose}/>
    <details class="avatar-presets" bind:this={menu}>
      <summary aria-label={`Choose default profile image for ${name}`} title="Default profile pictures" onclick={event=>{if(disabled||busy)event.preventDefault();}}><span aria-hidden="true">⌄</span></summary>
      <div class="portrait-menu"><strong>Default pictures</strong><span>AI generated</span><div class="portrait-options">{#each DEFAULT_PROFILE_IMAGES as portrait}<button type="button" aria-label={`Use ${portrait.label.toLowerCase()} portrait for ${name}`} aria-pressed={displayedImage===portrait.src} disabled={disabled||busy} onclick={()=>usePreset(portrait.src)}><img src={portrait.src} alt=""/><span>{portrait.label}</span></button>{/each}</div></div>
    </details>
    {#if error}<p class="avatar-error" role="alert">{error}</p>{/if}
  </div>
{:else}<span class="profile-avatar" style={`--avatar-size:${size}px`} aria-hidden="true">{@render picture()}</span>{/if}
<style>
  .avatar-presets{position:absolute;left:-3px;bottom:-2px;z-index:2}.avatar-presets>summary{display:grid;place-items:center;list-style:none;width:20px;height:20px;border:2px solid var(--studio-panel);border-radius:50%;background:var(--studio-accent);color:var(--studio-tone-04130f);font-size:15px;line-height:1;cursor:pointer}.avatar-presets>summary::-webkit-details-marker{display:none}.portrait-menu{position:absolute;left:0;top:calc(100% + 8px);width:190px;padding:12px;border:1px solid var(--studio-border);border-radius:12px;background:var(--studio-panel);box-shadow:0 8px 24px var(--studio-tone-0008);color:var(--studio-text)}.portrait-menu>strong{display:block;font-size:12px}.portrait-menu>span{display:block;margin-top:3px;font-size:10px;color:var(--studio-muted)}.portrait-options{display:flex;gap:8px;margin-top:10px}.portrait-options button{display:flex;flex:1;flex-direction:column;align-items:center;gap:6px;padding:7px;border:1px solid transparent;border-radius:9px;background:transparent;color:var(--studio-text);font-size:11px;font-weight:400}.portrait-options button:hover,.portrait-options button[aria-pressed=true]{border-color:var(--studio-accent);background:color-mix(in srgb,var(--studio-accent) 9%,var(--studio-panel))}.portrait-options img{width:56px;height:56px;border-radius:50%;object-fit:cover}.avatar-presets>summary:focus-visible{outline:2px solid var(--studio-focus);outline-offset:3px}

  .avatar-upload{position:relative;flex:none}.profile-avatar{position:relative;display:grid;place-items:center;width:var(--avatar-size);height:var(--avatar-size);flex:none;padding:0;border:1px solid color-mix(in srgb,var(--studio-accent) 35%,var(--studio-border));border-radius:50%;background:color-mix(in srgb,var(--studio-accent) 13%,var(--studio-panel));color:var(--studio-accent)}.profile-avatar img{display:block;width:100%;height:100%;object-fit:cover;border-radius:50%}.editable:hover{border-color:var(--studio-accent);box-shadow:0 0 0 3px color-mix(in srgb,var(--studio-accent) 15%,transparent)}.avatar-edit{position:absolute;right:-2px;bottom:-2px;display:grid;place-items:center;width:19px;height:19px;border-radius:50%;background:var(--studio-accent);color:var(--studio-tone-04130f);border:2px solid var(--studio-panel);font-size:13px;line-height:1;font-weight:600}.avatar-error{position:absolute;left:0;top:100%;z-index:3;width:220px;margin:7px 0 0;padding:10px;border:1px solid var(--studio-border);border-radius:8px;background:var(--studio-panel);color:var(--studio-tone-ffb2be);font-size:11px;line-height:1.6}.editable:focus-visible{outline:2px solid var(--studio-focus);outline-offset:3px}
</style>
