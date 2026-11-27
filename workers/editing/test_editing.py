from pathlib import Path
import tempfile
import unittest
import numpy as np
from audio import PCM,observe_splices,splice_result,preserved
from engine import Projects,Refused,window_for,shift_regions,change_audio,validate_words,atomic_json


class AudioPreservation(unittest.TestCase):
    def test_every_pcm_depth_and_channel_count_preserves_untouched_bytes(self):
        for width in (1,2,3,4):
            for channels in (1,2,6):
                with self.subTest(width=width,channels=channels):
                    original=PCM(48000,channels,width,b'')
                    values=np.linspace(-.8,.8,96000,dtype=np.float32)
                    original=PCM(48000,channels,width,original.encode(values))
                    edited={'audio':np.linspace(-.2,.3,48000,dtype=np.float32)}
                    events=[{'input':[12000,14400],'output':[12000,15600]}]
                    result,marks=splice_result(original,edited,events)
                    self.assertTrue(preserved(original,result,marks))
                    self.assertEqual(result.frames-original.frames,2400)
                    changed=bytearray(result.data);changed[0]^=1
                    self.assertFalse(preserved(original,PCM(result.rate,channels,width,bytes(changed)),marks))

    def test_splice_observer_covers_fades_and_delete_seams(self):
        class Editor:
            def _edit_one(self,a):
                out=np.concatenate([a[:100],np.full(20,.9,np.float32),a[130:]])
                return out,{'span_in':[110/24000,120/24000]}
        ed=Editor();original,events=observe_splices(ed)
        a=np.arange(300,dtype=np.float32)/300
        ed._edit_one(a)
        self.assertEqual(events,[{'input':[100,130],'output':[100,120]}])
        ed._edit_one=original

    def test_two_splices_keep_all_unchanged_islands(self):
        pcm=PCM(24000,1,2,b'');pcm=PCM(24000,1,2,pcm.encode(np.linspace(-.7,.7,1000)))
        out=np.zeros(1020,np.float32)
        result,marks=splice_result(pcm,{'audio':out},[
            {'input':[100,200],'output':[100,220]}, {'input':[700,800],'output':[720,820]}])
        self.assertTrue(preserved(pcm,result,marks))
        self.assertEqual(result.frames,1020)


class WindowsAndHistory(unittest.TestCase):
    def test_long_recording_window_has_context_and_pause_boundaries(self):
        words=[{'word':'ሰላም','start':i+.1,'end':i+.8} for i in range(80)]
        a,b,lo,hi=window_for(words,40,42,80)
        self.assertLessEqual(b-a,30)
        self.assertGreaterEqual(words[40]['start']-a,6)
        self.assertGreaterEqual(b-words[41]['end'],6)
        self.assertLessEqual(lo,40);self.assertGreaterEqual(hi,42)
        with self.assertRaises(Refused):window_for(words,20,55,80)

    def test_region_shift_maps_replacements_and_insertions(self):
        edits=[{'span_in':[2,3],'span_out':[2,5]},{'span_in':[6,6],'span_out':[8,9]}]
        result=shift_regions([{'span_out':[7,8]},{'span_out':[2.5,2.8]}],edits)
        self.assertEqual(result[0]['span_out'],[10,11])
        self.assertAlmostEqual(result[1]['span_out'][0],3.5)

    def test_chained_regions_keep_sample_coordinates_in_sync(self):
        original=[{'span_out':[7,8],'frames_out':[336000,384000],'seed':41}]
        edits=[{'span_in':[2,3],'span_out':[2,5]}]
        shifted=shift_regions(original,edits,48000)
        self.assertEqual(shifted[0]['span_out'],[9,10])
        self.assertEqual(shifted[0]['frames_out'],[432000,480000])
        self.assertEqual(original[0]['frames_out'],[336000,384000])
        self.assertEqual(shifted[0]['seed'],41)

    def test_fix_undo_export_and_stale_revision_preserve_original(self):
        with tempfile.TemporaryDirectory() as d:
            root=Path(d);source=root/'input.wav'
            pcm=PCM(24000,1,2,b'\0\0'*24000);pcm.write(source)
            original=source.read_bytes();store=Projects(root/'projects')
            p=store.create(source,'input.wav','ሰላም',[{'word':'ሰላም','start':0,'end':1}])
            before=store.audio(p).data
            p=store.commit(p['id'],0,'ጤና',[{'word':'ጤና','start':0,'end':1}])
            self.assertEqual(store.audio(p).data,before)
            with self.assertRaises(ValueError):store.commit(p['id'],0,'stale',[])
            store=Projects(root/'projects')
            p=store.undo(p['id'],1)
            self.assertEqual(p['versions'][p['head']]['text'],'ሰላም')
            self.assertEqual(source.read_bytes(),original)
            self.assertFalse(store.export_log(p['id'])['synthetic'])

    def test_empty_target_is_a_reversible_full_cut(self):
        pcm=PCM(24000,1,2,b'\0\0'*24000)
        out,words,marks=change_audio(None,pcm,'ሰላም','',[{'word':'ሰላም','start':0,'end':1}],1,16)
        self.assertEqual(out.frames,0);self.assertEqual(words,[])
        self.assertTrue(preserved(pcm,out,marks))

    def test_invalid_alignment_is_rejected(self):
        with self.assertRaises(Refused):validate_words([{'word':'ሰላም','start':float('nan'),'end':1}],'ሰላም',2)
        with self.assertRaises(Refused):validate_words([],'ሰላም',2)

    def test_history_limit_never_replaces_the_last_readable_revision(self):
        with tempfile.TemporaryDirectory() as d:
            path=Path(d)/'project.json';atomic_json(path,{'revision':4})
            before=path.read_bytes()
            with self.assertRaises(Refused) as caught:
                atomic_json(path,{'revision':5,'history':'x'*(16*1024*1024)})
            self.assertEqual(caught.exception.kind,'history_full')
            self.assertEqual(path.read_bytes(),before)

    def test_word_change_can_also_update_punctuation_without_reencoding_other_words(self):
        class Editor:
            _studio_norm=staticmethod(lambda t:t.rstrip('!'))
            def _edit_one(self,a):
                out=a.copy();out[12000:15000]=.5
                return out,{}
            def edit(self,a,old,new,**kwargs):
                result,_=self._edit_one(a)
                return {'audio':result,'words':kwargs['words']}
            def align(self,a,text,sr):
                return [{'word':t,'start':i*.5,'end':(i+1)*.5} for i,t in enumerate(text.split())]
        source=PCM(24000,1,2,b'\0\0'*48000)
        words=[{'word':'ሰላም','start':0,'end':.5},{'word':'ዛሬ','start':.5,'end':1}]
        out,aligned,marks=change_audio(Editor(),source,'ሰላም ዛሬ','ጤና ዛሬ!',words,1,16)
        self.assertEqual([w['word'] for w in aligned],['ጤና','ዛሬ!'])
        self.assertTrue(preserved(source,out,marks))


if __name__=='__main__':unittest.main()
