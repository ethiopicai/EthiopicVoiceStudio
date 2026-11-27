<script>
  import {onMount} from 'svelte';
  import TimedTranscript from '../../src/TimedTranscript.svelte';
  import PlaybackHighlightControl from '../../src/PlaybackHighlightControl.svelte';
  let tokens=$state([{id:'unsafe[]"id',text:'<script>safe<'+'/script>',start:0,end:.4},{text:'ሰላም',start:.6,end:1},{id:900,text:'third',start:1,end:1.5}]);
  let position=$state(0),playing=$state(false),highlight=$state(true),otherHighlight=$state(false),enabled=$state(true),selected=$state(null),longHighlight=$state(false);
  const longTokens=Array.from({length:100},(_,index)=>({text:`word-${index}`,start:index,end:index+.5}));
  onMount(()=>{window.timedFixture={setClock:(seconds,running)=>{position=seconds;playing=running;},setEnabled:value=>enabled=value,replace:values=>tokens=values,selection:()=>selected,followLast:()=>{longHighlight=true;position=99.1;playing=true;}};return()=>delete window.timedFixture;});
</script>
<main>
  <button id="clock-focus">Keep keyboard focus here</button>
  <section id="primary"><TimedTranscript {tokens} {position} {playing} {enabled} {highlight} selectedIndex={selected} label="External player transcript" language="am" onSelect={(_token,index)=>selected=index} text="No timestamps"/><PlaybackHighlightControl bind:enabled={highlight}/></section>
  <section id="secondary"><TimedTranscript {tokens} {position} {playing} highlight={otherHighlight} label="Independent read-only transcript" language="am"/><PlaybackHighlightControl bind:enabled={otherHighlight} label="Highlight other transcript"/></section>
  <section id="long"><TimedTranscript tokens={longTokens} {position} {playing} highlight={longHighlight} label="Long transcript"/></section>
</main>
<style>
main{max-width:700px;color:#e9eefb;background:#0d1728;padding:20px;font-family:system-ui}section{max-width:350px;--timed-transcript-max-height:92px;--timed-transcript-font-size:18px}#long{max-width:210px;margin-top:25px}button{padding:8px}
</style>
