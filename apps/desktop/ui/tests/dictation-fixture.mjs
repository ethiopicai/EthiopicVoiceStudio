export function seedDictation(historyCount=0){
      window.calls=[];window.microphoneTracks=[];window.session=null;window.totalSamples=0;window.tail='first-tail';window.saved=[];window.pending=[];window.count=0;window.exports=[];window.clipboard='';
      Object.defineProperty(navigator,'clipboard',{value:{writeText:async value=>window.clipboard=value},configurable:true});
      const make=(id)=>({id,name:`Dictation-${id}.wav`,text:`fixed last-word-${id}`,model_version:'v0',created_at_ms:Date.now(),revision:1,words:[{id:0,word:'fixed',corrected:'fixed',start:0,end:.1},{id:1,word:`last-word-${id}`,corrected:`last-word-${id}`,start:.3,end:.4}],corrections:[]});
      window.saved=Array.from({length:historyCount},(_,index)=>({...make(`old-${index+1}`),created_at_ms:index+1})).reverse();
      const wav=()=>{const b=new Uint8Array(32044),v=new DataView(b.buffer);for(const [at,text] of [[0,'RIFF'],[8,'WAVE'],[12,'fmt '],[36,'data']])for(let i=0;i<text.length;i++)v.setUint8(at+i,text.charCodeAt(i));v.setUint32(4,b.length-8,true);v.setUint32(16,16,true);v.setUint16(20,1,true);v.setUint16(22,1,true);v.setUint32(24,16000,true);v.setUint32(28,32000,true);v.setUint16(32,2,true);v.setUint16(34,16,true);v.setUint32(40,32000,true);return Array.from(b);};
      const get=navigator.mediaDevices.getUserMedia.bind(navigator.mediaDevices);
      navigator.mediaDevices.getUserMedia=async options=>{window.calls.push('microphone');const stream=await get(options);window.microphoneTracks.push(...stream.getTracks());return stream;};
      window.__TAURI_INTERNALS__={invoke:async(command,args)=>{
        window.calls.push(command);
        if(command==='get_asr_status')return {state:'ready',message:'Ready'};
        if(command==='get_fixture_progress')return {state:'idle',downloadedBytes:0,totalBytes:0};
        if(command==='get_active_dictation_session')return null;
        if(command==='start_dictation'){window.session={id:`live-${++window.count}`,state:'ready',stage:'Ready',modelVersion:'v0',decodeSeconds:.5,finalWords:[],partialWords:[]};return window.session;}
        if(command==='feed_dictation'){window.totalSamples+=args.samples.length;window.session.state='recording';window.session.finalWords=[{word:'fixed',start:0,end:.1}];window.session.partialWords=[{word:window.tail,start:.2,end:.3}];return;}
        if(command==='get_dictation_session')return window.session;
        if(command==='stop_dictation'){const id=window.session.id;window.saved.push(make(id));window.session={...window.session,state:'completed',stage:'Saved',transcriptId:id,partialWords:[],finalWords:[...window.session.finalWords,{word:`last-word-${id}`,start:.3,end:.4}]};return;}
        if(command==='cancel_dictation'){window.session.state='cancelled';return;}
        if(command==='import_transcription_audio'){window.importSize=args.bytes.length;return {id:'imported'};}
        if(command==='start_transcription'){const id=`batch-${++window.count}`;window.saved.push({...make(id),source_recording_id:args.audioId});return {id,state:'running',stage:'Recognizing'};}
        if(command==='get_transcription_job')return {id:args.jobId,state:'completed',stage:'Saved',transcriptId:args.jobId};
        if(command==='list_transcripts')return window.saved;
        if(command==='get_transcript')return window.saved.find(t=>t.id===args.id);
        if(command==='correct_transcript_word'){
          const transcript=window.saved.find(t=>t.id===args.id);transcript.words[args.wordId].corrected=args.text;transcript.text=transcript.words.map(w=>w.corrected).filter(Boolean).join(' ');transcript.revision++;return transcript;
        }

        if(command==='copy_transcript_text'){window.clipboard=window.saved.find(t=>t.id===args.id).text;return;}
        if(command==='copy_transcript_wav_path'){window.clipboard=`/mock/${args.id}.wav`;return;}
        if(command==='get_transcript_wav_path')return `/mock/${args.id}.wav`;
        if(command==='export_transcript'){window.exports.push({id:args.id,format:args.format});return {name:`dictation.${args.format}`,path:`/mock/dictation.${args.format}`,message:'Saved'};}
        if(command==='delete_transcript'){window.saved=window.saved.filter(t=>t.id!==args.id);return [];}

        if(command==='get_transcript_audio')return {name:`Dictation-${args.id}.wav`,bytes:wav()};
        if(command==='list_transcription_recordings')return window.pending.filter(p=>!window.saved.some(t=>t.source_recording_id===p.id));
        if(command==='get_transcription_recording_audio')return {name:'retry.wav',bytes:wav()};
        throw Error('Outside this test');
      }};
}
