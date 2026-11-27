<script lang="ts">
  let {enabled=$bindable(true),disabled=false,label='Highlight words during playback'}:{enabled?:boolean;disabled?:boolean;label?:string}=$props();
</script>
<label class="playback-highlight-control"><input type="checkbox" bind:checked={enabled} {disabled}/>{label}</label>
<style>
.playback-highlight-control{display:flex;align-items:center;gap:9px;padding-top:10px;margin:0;font-size:12px;line-height:1.5;color:var(--studio-tone-a9bad2);cursor:pointer}.playback-highlight-control input{accent-color:var(--studio-accent);width:16px;height:16px;flex:none;margin:0}.playback-highlight-control input:focus-visible{outline:2px solid var(--studio-gold);outline-offset:3px}
</style>
