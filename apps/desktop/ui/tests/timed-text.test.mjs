import test from 'node:test';
import assert from 'node:assert/strict';
import {timedTokenIndexAt,validTimedSequence} from '../src/timed-text.js';

test('generic timed text uses indices independently of model/project identifiers',()=>{
 const tokens=[{id:'arbitrary-id',text:'first',start:.2,end:.6},{text:'second',start:.8,end:1.2},{id:900,text:'third',start:1.2,end:2}];
 assert.equal(validTimedSequence(tokens),true);
 for(const [at,index] of [[0,null],[.2,0],[.599,0],[.6,null],[.8,1],[1.2,2],[2,null],[.3,0]])assert.equal(timedTokenIndexAt(tokens,at),index);
});
test('bad clocks and timing sequences do not produce false highlights',()=>{
 for(const at of [NaN,Infinity,-1])assert.equal(timedTokenIndexAt([{start:0,end:1}],at),null);
 for(const tokens of [[{start:-1,end:1}],[{start:2,end:1}],[{start:NaN,end:1}],[{start:0,end:Infinity}],[{start:1,end:2},{start:0,end:1}],[{start:0,end:2},{start:1,end:3}]])assert.equal(validTimedSequence(tokens),false);
 assert.equal(timedTokenIndexAt([],0),null);
 assert.equal(validTimedSequence([{start:0,end:0},{start:0,end:1}]),true);
 assert.equal(timedTokenIndexAt([{start:0,end:0},{start:0,end:1}],0),1);
});
