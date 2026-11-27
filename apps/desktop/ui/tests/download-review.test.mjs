import test from 'node:test';
import assert from 'node:assert/strict';
import {downloadSize} from '../src/download-review.js';
test('download sizes use decimal units and do not disguise an unknown size as zero',()=>{
 assert.equal(downloadSize(120000000),'120.0 MB');
 assert.equal(downloadSize(2500000000),'2.50 GB');
 assert.equal(downloadSize(1580),'1.6 KB');
 assert.equal(downloadSize(20),'20 B');
 for(const value of [NaN,undefined,-1,Infinity])assert.equal(downloadSize(value),'Size unavailable');
});
