export function seedVoiceHistory(extra=0){
    if(window.self!==window.top)return;
    const bytes=new Uint8Array(32044),v=new DataView(bytes.buffer);
    for(const [at,text] of [[0,'RIFF'],[8,'WAVE'],[12,'fmt '],[36,'data']])for(let i=0;i<text.length;i++)v.setUint8(at+i,text.charCodeAt(i));
    v.setUint32(4,bytes.length-8,true);v.setUint32(16,16,true);v.setUint16(20,1,true);v.setUint16(22,1,true);v.setUint32(24,16000,true);v.setUint32(28,32000,true);v.setUint16(32,2,true);v.setUint16(34,16,true);v.setUint32(40,32000,true);
    window.voiceBytes=Array.from(bytes);window.voiceCalls=[];window.deleteRequests=[];window.failRequest=false;window.failId='';window.voiceBusy=false;
    window.cleanupSources=[{id:'input-one',name:'Original one.wav'},{id:'input-two',name:'Original two.wav'}];window.cleanupResults=[{id:'result-one',name:'Cleaned one.wav',sample_rate:16000,channels:1,frames:16000}];
    const project=(id,name,revision)=>({id,name,revision,created:1,head:0,duration:1,channels:1,versions:[{audio:'original.wav',text:'ጤና',words:[],regions:[],parent:null,kind:'import'}]});
    window.voiceProjects=[project('edit-one','Edit one.wav',3),project('edit-two','Edit two.wav',0),project('generation-one','Generated one.wav',0)];window.voiceGenerations=[{id:'generation-one',name:'Generated one.wav',text:'Saved speech',revision:0,created:1}];
    const profiles=[{id:'profile-one',name:'Voice one',kind:'reference-draft',language:'am-ET',reference:new File([bytes],'one.wav',{type:'audio/wav'}),referenceText:'ጤና'},{id:'profile-two',name:'Voice two',kind:'reference-draft',language:'am-ET',reference:new File([bytes],'two.wav',{type:'audio/wav'}),referenceText:'ጤና'}];
    for(let i=0;i<extra;i++){window.cleanupSources.push({id:`extra-${i}`,name:`Recording ${i}.wav`});window.voiceProjects.push(project(`extra-${i}`,`Recording ${i}.wav`,0));window.voiceGenerations.push({id:`extra-${i}`,name:`Speech ${i}.wav`,text:`Saved speech ${i}`,revision:0,created:1});profiles.push({...profiles[0],id:`extra-${i}`,name:`Profile ${i}`});}
    const database=indexedDB.open('ethiopic-voice-profiles',2);database.onupgradeneeded=()=>{const store=database.result.createObjectStore('profiles',{keyPath:'id'});for(const profile of profiles)store.put(profile);database.result.createObjectStore('presetPacks',{keyPath:'id'}).put({id:'atts1hg1-am-preview-v3'});};database.onsuccess=()=>database.result.close();
    window.__TAURI_INTERNALS__={invoke:async(command,args={})=>{
     window.voiceCalls.push(command);
     if(command==='system_summary')return {os:'macos',arch:'aarch64',logicalCpus:8,totalMemoryBytes:16*1024**3};
     if(command==='get_asr_status'||command==='get_cleanup_status')return {state:'ready',message:'Ready'};
     if(command==='get_active_dictation_session'||command==='get_active_cleanup_job')return null;
     if(command==='get_voice_generation_status')return {ready:false,consent:true,busy:window.voiceBusy,requiresReferenceText:false,defaultVoice:true,canPrepare:true,reason:'Download speech tools.'};
     if(command==='get_voice_edit_status')return {selection:{available:false,opt_in_required:false,slow:false,reason:'Editing unavailable on fixture hardware.',tier:'apple-arm',download_bytes:0,licenses:[]},consent:true,optIn:false,ready:false,runtimePresent:false,busy:window.voiceBusy,stage:'',loaded:false};
     if(command==='get_voice_edit_busy')return window.voiceBusy;
     if(command==='list_transcripts')return [];
     if(command==='list_cleanup_recordings')return structuredClone(window.cleanupSources);
     if(command==='list_cleanup_results')return structuredClone(window.cleanupResults);
     if(command==='list_voice_edit_projects')return structuredClone(window.voiceProjects);
     if(command==='list_voice_generations')return structuredClone(window.voiceGenerations);
     if(command==='get_cleanup_recording_audio')return {name:'Original one.wav',bytes:window.voiceBytes};
     if(command==='get_cleanup_audio')return {name:'Cleaned one.wav',bytes:window.voiceBytes};
     if(command==='get_speech_versions'){const item=window.voiceGenerations.find(item=>item.id===args.groupId);return item?[{projectId:item.id,text:item.text,settings:item.settings}]:[];}
     if(command==='resolve_recording_transfer')return {name:'Saved reference.wav',bytes:window.voiceBytes,context:{schemaVersion:1,source:{kind:'editing',id:args.input.id},audioSha256:'fixture',text:'ጤና',words:[],timingsCurrent:false,editedRegions:[],lineage:[]}};
     if(command==='get_voice_edit_audio')return {name:'Audio.wav',bytes:window.voiceBytes};
     if(command==='delete_voice_history'){
      window.deleteRequests.push(structuredClone(args.items));if(window.failRequest)throw 'Revision conflict or active processing';
      const result={deleted:[],failed:[]};
      for(const item of args.items){
       if(item.id===window.failId){result.failed.push({...item,message:'Could not delete'});continue;}
       result.deleted.push({kind:item.kind,id:item.id});
       if(item.kind==='cleanup-recording')window.cleanupSources=window.cleanupSources.filter(source=>source.id!==item.id);
       if(item.kind==='cleanup-result')window.cleanupResults=window.cleanupResults.filter(source=>source.id!==item.id);
       if(item.kind==='voice-project'){window.voiceProjects=window.voiceProjects.filter(project=>project.id!==item.id);window.voiceGenerations=window.voiceGenerations.filter(project=>project.id!==item.id);}
      }
      return result;
     }
     throw Error('Outside this test');
    }};
}
