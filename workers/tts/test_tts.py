import importlib.util,json,sys,tempfile,unittest
from pathlib import Path
import numpy as np
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'editing'))
from engine import Projects
from audio import PCM
spec=importlib.util.spec_from_file_location('small_tts_worker',Path(__file__).with_name('worker.py'));worker=importlib.util.module_from_spec(spec);spec.loader.exec_module(worker)
class CpuSpeech(unittest.TestCase):
 def test_output_is_separate_pcm_and_revision_errors_leave_originals_unchanged(self):
  with tempfile.TemporaryDirectory() as d:
   root=Path(d);source=root/'reference.wav';PCM(24000,1,2,b'\0\0'*24000).write(source);original=source.read_bytes()
   model={'hf_revision':'pinned','variants':[{'id':'tts-flow_lm-90m-amh-fp32','model_version':'v0.1'}]};provider=worker.Provider(root,root,model)
   class Fake:
    sample_rate=24000;_voices={};model=type('Model',(),{'temp':.3})()
    def _voice_state(self,voice):pass
    def synth(self,text,voice):return np.full(24000,.002,np.float32)
   provider.tts=Fake();project=provider.projects.create(source,'Reference');request={'action':'synthesize','projectId':project['id'],'revision':0,'text':'ሰላም።'}
   result=provider.dispatch(request);output=provider.projects.folder(project['id'])/'synthesis.wav';saved=output.read_bytes()
   self.assertEqual(result['model_version'],'v0.1');self.assertEqual(PCM.read(output).rate,24000);self.assertEqual(source.read_bytes(),original)
   self.assertEqual(provider.projects.get(project['id'])['revision'],0)
   for bad in ({**request,'revision':3},{**request,'text':''},{**request,'text':'x'*12001},{**request,'action':'preview'}):
    with self.assertRaises(ValueError):provider.dispatch(bad)
    self.assertEqual(output.read_bytes(),saved)
 def test_nonfinite_audio_does_not_replace_a_saved_take(self):
  with tempfile.TemporaryDirectory() as d:
   root=Path(d);source=root/'reference.wav';PCM(24000,1,2,b'\0\0'*24000).write(source);provider=worker.Provider(root,root,{'hf_revision':'pin','variants':[]});project=provider.projects.create(source,'Voice');output=provider.projects.folder(project['id'])/'synthesis.wav';output.write_bytes(b'previous take')
   class Fake:
    sample_rate=24000;_voices={};model=type('Model',(),{'temp':.3})()
    def _voice_state(self,voice):pass
    def synth(self,text,voice):return np.array([np.nan],np.float32)
   provider.tts=Fake()
   with self.assertRaises(ValueError):provider.dispatch({'action':'synthesize','projectId':project['id'],'revision':0,'text':'ሰላም።'})
   self.assertEqual(output.read_bytes(),b'previous take')
 def test_controls_use_seed_and_temperature_add_exact_pauses_and_restore_resident_state(self):
  import torch
  with tempfile.TemporaryDirectory() as d:
   root=Path(d);source=root/'reference.wav';PCM(24000,1,2,b'\0\0'*24000).write(source)
   provider=worker.Provider(root,root,{'hf_revision':'pin','variants':[{'id':'tts-flow_lm-90m-amh-fp32','model_version':'v0.1'}]})
   class Fake:
    sample_rate=24000;_voices={};model=type('Model',(),{'temp':.3})()
    def _voice_state(self,voice):
     if voice not in self._voices:self._voices[voice]=torch.rand(1)
    def synth(self,text,voice):return (torch.rand(100)*self.model.temp).numpy()
   provider.tts=Fake();provider.sentences=lambda text:text.split('|');project=provider.projects.create(source,'Voice')
   request={'action':'synthesize','projectId':project['id'],'revision':0,'text':'one|two','seed':42,'generationOptions':{'temperature':.6,'sentencePause':.5}}
   state=torch.random.get_rng_state().clone();result=provider.dispatch(request);path=provider.projects.folder(project['id'])/'synthesis.wav';first=path.read_bytes()
   self.assertTrue(torch.equal(state,torch.random.get_rng_state()));self.assertEqual(provider.tts.model.temp,.3)
   self.assertEqual(result['settings'],{'schemaVersion':1,'temperature':.6,'sentencePause':.5,'seed':42})
   pcm=PCM.read(path);self.assertEqual(pcm.frames,200+12000);self.assertEqual(pcm.data[200:24200],b'\0'*(12000*2))
   provider.dispatch(request);self.assertEqual(first,path.read_bytes())
   provider.dispatch({**request,'seed':43});self.assertNotEqual(first,path.read_bytes())
   provider.dispatch({**request,'generationOptions':{'temperature':.2,'sentencePause':.5}});self.assertNotEqual(first,path.read_bytes())
   provider.dispatch({**request,'generationOptions':{'temperature':.6,'sentencePause':0}});self.assertEqual(PCM.read(path).frames,200)
   provider.tts.synth=lambda *args,**kwargs:np.array([np.nan],np.float32)
   with self.assertRaises(ValueError):provider.dispatch(request)
   self.assertEqual(provider.tts.model.temp,.3);self.assertTrue(torch.equal(state,torch.random.get_rng_state()))
 def test_invalid_controls_reject_before_loading(self):
  with tempfile.TemporaryDirectory() as d:
   provider=worker.Provider(Path(d),Path(d),{})
   for options in ({'temperature':float('nan')},{'temperature':True},{'temperature':0},{'sentencePause':None},{'sentencePause':3},{'speed':1},[],False):
    with self.subTest(options=options),self.assertRaises(ValueError):provider.dispatch({'action':'synthesize','generationOptions':options})
   for seed in (True,-1,4294967296,1.5):
    with self.assertRaises(ValueError):provider.dispatch({'action':'synthesize','seed':seed})
   self.assertIsNone(provider.tts)
if __name__=='__main__':unittest.main()
