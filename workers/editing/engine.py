"""Bounded editing windows, PCM-preserving edits, and an atomic local project history."""
import copy
import difflib
import hashlib
import json
import math
import os
from pathlib import Path
import re
import secrets
import time

from audio import PCM, observe_splices, splice_result, preserved

ERRORS = {
    'no_change': 'Change a spoken word before regenerating. Use Fix transcript for punctuation or spelling only.',
    'text_mismatch': 'The transcript and word timings disagree. Fix the transcript and align it again.',
    'alignment_failed': 'Some words could not be located. Check that the transcript matches the recording.',
    'too_long': 'This change needs a smaller selection. Change a shorter phrase so six seconds of context fit.',
    'too_short': 'At least half a second of audio is needed. Choose a longer recording.',
    'bad_num_step': 'The voice tools have an incompatible quality setting. Repair voice tools and try again.',
    'empty_text': 'No speech was found. Check the recording or enter a matching transcript.',
    'history_full': 'This project history is full. Export the current recording and start a new editing project.',
}


class Refused(ValueError):
    def __init__(self, kind):
        self.kind = kind
        super().__init__(ERRORS.get(kind, 'Voice editing could not finish. Your saved recording is unchanged.'))


def checked_text(value):
    if not isinstance(value, str) or len(value) > 200000 or '\x00' in value:
        raise ValueError('Invalid transcript')
    return ' '.join(value.split())


def validate_words(words, text, duration):
    if not isinstance(words,list) or len(words) != len(text.split()):
        raise Refused('text_mismatch')
    last = 0
    for token,w in zip(text.split(),words):
        if w.get('word') != token:
            raise Refused('text_mismatch')
        start,end = w.get('start'),w.get('end')
        if start is None and end is None:
            continue
        if (not isinstance(start,(int,float)) or not isinstance(end,(int,float))
                or not math.isfinite(start) or not math.isfinite(end)
                or start < last-0.025 or end < start or end > duration+0.05):
            raise Refused('alignment_failed')
        last=end


def timed(words):
    last=0.0; result=[]
    for w in words:
        item=dict(w)
        if item.get('start') is None or item.get('end') is None:
            item.update(start=last,end=last,aligned=False)
        else:
            item['aligned']=item.get('aligned',True)
            last=item['end']
        result.append(item)
    return result


def window_for(words, i, j, duration):
    w=timed(words)
    a=w[i]['start'] if i<len(w) else duration
    b=w[j-1]['end'] if j>i else a
    if i==j and i:
        a=b=(w[i-1]['end']+(w[i]['start'] if i<len(w) else duration))/2
    left=max(0,a-6.05); right=min(duration,b+6.05)
    boundaries=[(0.0,0)]+[((w[k-1]['end']+w[k]['start'])/2,k) for k in range(1,len(w))]+[(duration,len(w))]
    start,lo=max((p for p in boundaries if p[0]<=left+1e-6),key=lambda p:p[0])
    end,hi=min((p for p in boundaries if p[0]>=right-1e-6),key=lambda p:p[0])
    if end-start>30 or end-start<0.5:
        raise Refused('too_long' if end-start>30 else 'too_short')
    return start,end,lo,hi


def align_project(ed, pcm, text, coarse=None):
    text=checked_text(text)
    if not text:
        return []
    if pcm.duration<=30:
        result=ed.align(pcm.mono(),text,sr=pcm.rate)
        validate_words(result,text,pcm.duration)
        return timed(result)
    if coarse is None or len(coarse)!=len(text.split()):
        # ASR anchors provide window boundaries only; the corrected text is force-aligned below.
        raw=ed.transcribe(pcm.mono(),sr=pcm.rate)
        anchors=timed(raw['words'])
        matcher=difflib.SequenceMatcher(a=raw['text'].split(),b=text.split(),autojunk=False)
        coarse=[]
        for op,i,j,a,b in matcher.get_opcodes():
            if op=='equal': coarse.extend(dict(w,word=t) for w,t in zip(anchors[i:j],text.split()[a:b]))
            else:
                start=anchors[i]['start'] if i<len(anchors) else pcm.duration
                end=anchors[j-1]['end'] if j>i else start
                coarse.extend({'word':t,'start':start+(end-start)*k/max(1,b-a),
                               'end':start+(end-start)*(k+1)/max(1,b-a)} for k,t in enumerate(text.split()[a:b]))
    tokens=text.split(); coarse=timed(coarse); result=[]; lo=0; start=0.0
    while lo<len(tokens):
        hi=lo+1
        while hi<len(tokens) and coarse[hi]['end']-start<25:
            hi+=1
        end=pcm.duration if hi==len(tokens) else (coarse[hi-1]['end']+coarse[hi]['start'])/2
        if end-start>30 or end<=start:
            raise Refused('alignment_failed')
        clip=pcm.slice(round(start*pcm.rate),round(end*pcm.rate))
        aligned=ed.align(clip.mono(),' '.join(tokens[lo:hi]),sr=pcm.rate)
        for item in aligned:
            item=dict(item)
            if item.get('start') is not None:
                item.update(start=item['start']+start,end=item['end']+start)
            result.append(item)
        lo,start=hi,end
    validate_words(result,text,pcm.duration)
    return timed(result)


def change_audio(ed, pcm, orig_text, new_text, words, seed, steps, progress=lambda _:None):
    orig_text,new_text=checked_text(orig_text),checked_text(new_text)
    validate_words(words,orig_text,pcm.duration)
    if not new_text:
        return PCM(pcm.rate,pcm.channels,pcm.width,b''),[],[{
            'span_in':[0,pcm.duration],'span_out':[0,0], 'frames_in':[0,pcm.frames],'frames_out':[0,0],
            'orig_words':orig_text.split(),'new_words':[]}]
    # Match the packaged editor's spoken-word equivalence, not punctuation-only changes.
    norm=ed._studio_norm
    groups=[(i,j,a,b) for op,i,j,a,b in difflib.SequenceMatcher(
        a=[norm(t) for t in orig_text.split()],b=[norm(t) for t in new_text.split()],autojunk=False).get_opcodes() if op!='equal']
    merged=[]
    for i,j,a,b in groups:
        if merged and i-merged[-1][1]<=1 and a-merged[-1][3]<=1:
            previous=merged[-1];merged[-1]=(previous[0],j,previous[2],b)
        else:merged.append((i,j,a,b))
    groups=merged
    if not groups:
        raise Refused('no_change')
    current=pcm; current_words=timed(words); current_tokens=orig_text.split(); marks=[]
    # Each independent changed phrase gets a bounded context window; right-to-left preserves original indices.
    for n,(i,j,a,b) in enumerate(reversed(groups)):
        progress(f'Regenerating change {n+1} of {len(groups)}…')
        start,end,lo,hi=window_for(current_words,i,j,current.duration)
        first,last=round(start*pcm.rate),round(end*pcm.rate)
        clip=current.slice(first,last)
        before=' '.join(current_tokens[lo:hi])
        after_tokens=current_tokens[lo:i]+new_text.split()[a:b]+current_tokens[j:hi]
        after=' '.join(after_tokens)
        local=[dict(w,start=w['start']-start,end=w['end']-start) for w in current_words[lo:hi]]
        original,events=observe_splices(ed)
        try:
            out=ed.edit(clip.mono(),before,after,words=local,seed=int(seed),num_step=steps,sr=pcm.rate)
        finally:
            ed._edit_one=original
        if not events:
            raise Refused('no_change')
        replacement,regions=splice_result(clip,out,events)
        if not preserved(clip,replacement,regions):
            raise RuntimeError('PCM preservation check failed')
        delta=(replacement.frames-clip.frames)/pcm.rate
        current=PCM(pcm.rate,pcm.channels,pcm.width,current.data[:first*pcm.stride]+replacement.data+current.data[last*pcm.stride:])
        # Re-align only the edited window, using reference timings as coarse anchors if it grew beyond 30 seconds.
        aligned=align_project(ed,replacement,after,out['words'])
        current_words=current_words[:lo]+[dict(w,start=w['start']+start,end=w['end']+start) for w in aligned]+[
            dict(w,start=w['start']+delta,end=w['end']+delta) for w in current_words[hi:]]
        current_tokens=current_tokens[:lo]+after_tokens+current_tokens[hi:]
        for mark in marks:
            mark['span_out']=[v+delta for v in mark['span_out']]
            mark['frames_out']=[v+round(delta*pcm.rate) for v in mark['frames_out']]
        for region in regions:
            region=copy.deepcopy(region)
            for key in ['span_in','span_out']:region[key]=[v+start for v in region[key]]
            for key in ['frames_in','frames_out']:region[key]=[v+first for v in region[key]]
            region.update(orig_words=orig_text.split()[i:j],new_words=new_text.split()[a:b])
            marks.append(region)
    marks.sort(key=lambda m:m['span_in'][0])
    target_tokens=new_text.split()
    if (len(current_tokens)!=len(target_tokens)
            or any(norm(a)!=norm(b) for a,b in zip(current_tokens,target_tokens))
            or not preserved(pcm,current,marks)):
        raise RuntimeError('Edit preservation or transcript verification failed')
    # Spoken-equivalent punctuation/spelling changes outside the generated spans
    # still belong in the saved transcript, with the existing timing for that word.
    current_words=[dict(w,word=t) for w,t in zip(current_words,target_tokens)]
    validate_words(current_words,new_text,current.duration)
    return current,current_words,marks


def shift_regions(regions, edits, rate=None):
    """Map existing permanent annotations through insertions, deletions and replacements."""
    def point(t):
        delta=0.0
        for edit in sorted(edits,key=lambda e:e['span_in'][0]):
            a,b=edit['span_in']; c,d=edit['span_out']
            if t<a:break
            if t<=b and b>a:return c+(t-a)/(b-a)*(d-c)
            delta=d-b
        return t+delta
    result=[]
    for region in regions:
        item=dict(region,span_out=[point(v) for v in region['span_out']])
        if rate is not None:item['frames_out']=[round(v*rate) for v in item['span_out']]
        result.append(item)
    return result


def atomic_json(path,value):
    data=json.dumps(value,ensure_ascii=False,allow_nan=False).encode('utf-8')
    if len(data)>16*1024*1024:raise Refused('history_full')
    temp=path.with_suffix('.tmp')
    with temp.open('wb') as stream:
        stream.write(data)
        stream.flush();os.fsync(stream.fileno())
    temp.replace(path)


class Projects:
    def __init__(self,root):
        self.root=Path(root);self.root.mkdir(parents=True,exist_ok=True)

    def folder(self,identifier):
        if not isinstance(identifier,str) or not re.fullmatch('[a-zA-Z0-9-]{1,100}',identifier):
            raise ValueError('Invalid project identifier')
        p=self.root/identifier
        if p.is_symlink() or p.resolve()!=self.root.resolve()/identifier:
            raise ValueError('Redirected project storage')
        return p

    def get(self,identifier):
        p=self.folder(identifier)/'project.json'
        if p.is_symlink() or p.stat().st_size>16*1024*1024:raise ValueError('Invalid project metadata')
        return json.loads(p.read_text(encoding='utf-8'))

    def list(self):
        result=[]
        for p in self.root.iterdir():
            if p.is_dir() and not p.is_symlink():
                try:result.append(self.get(p.name))
                except (ValueError,OSError):pass
        return sorted(result,key=lambda p:p['created'],reverse=True)

    def create(self,source,name,text='',words=None):
        pcm=PCM.read(source)
        identifier=secrets.token_hex(12);p=self.folder(identifier);p.mkdir()
        # Immutable original bytes are copied, never moved or changed.
        (p/'original.wav').write_bytes(Path(source).read_bytes())
        project={'id':identifier,'name':str(name)[:240],'created':time.time(),'revision':0,'head':0,
                 'rate':pcm.rate,'channels':pcm.channels,'width':pcm.width,'duration':pcm.duration,
                 'versions':[{'audio':'original.wav','text':checked_text(text),'words':words or [],
                              'regions':[],'kind':'import','parent':None,'seed':None}], 'consent':False}
        self.save(project)
        return project

    def save(self,project):
        atomic_json(self.folder(project['id'])/'project.json',project)

    def audio(self,project):
        name=project['versions'][project['head']]['audio']
        if not re.fullmatch(r'(original|take-[a-f0-9]+)\.wav',name):raise ValueError('Invalid audio revision')
        path=self.folder(project['id'])/name
        if path.is_symlink():raise ValueError('Redirected project audio')
        return PCM.read(path)

    def commit(self,identifier,revision,text,words,audio=None,regions=None,kind='fix',seed=None,model=None):
        p=self.get(identifier)
        if p['revision']!=revision:raise ValueError('Project changed. Reopen it before saving.')
        old=p['versions'][p['head']]
        name=old['audio']
        if audio is not None:
            name='take-'+secrets.token_hex(12)+'.wav'
            audio.write(self.folder(identifier)/name)
        if regions is None:regions=old['regions']
        p['versions'].append({'audio':name,'text':text,'words':words,'regions':regions,
                              'parent':p['head'],'kind':kind,'seed':seed,'model':model,'created':time.time()})
        p['head']=len(p['versions'])-1;p['revision']+=1
        p['duration']=(audio or self.audio(p)).duration
        self.save(p);return p

    def undo(self,identifier,revision):
        p=self.get(identifier)
        if p['revision']!=revision:raise ValueError('Project changed. Reopen it first.')
        parent=p['versions'][p['head']]['parent']
        if parent is None:raise ValueError('No earlier edit to restore')
        p['head']=parent;p['revision']+=1;p['duration']=self.audio(p).duration
        self.save(p);return p

    def export_log(self,identifier):
        p=self.get(identifier);v=p['versions'][p['head']];history=[];head=p['head']
        while head is not None:
            item=p['versions'][head]
            history.append({k:val for k,val in item.items() if k!='audio'})
            head=item['parent']
        return {'schema_version':1,'project':p['id'],'name':p['name'],'synthetic':bool(v['regions']),
                'sample_rate':p['rate'],'channels':p['channels'],'text':v['text'],'words':v['words'],
                'regenerated_regions':v['regions'],'history':list(reversed(history)),
                'usage':'Non-commercial voice editing; reference voices require permission.'}
