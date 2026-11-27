import test from 'node:test';
import assert from 'node:assert/strict';
import {voiceSetupNotice, needsSetupOnLaunch} from '../src/voice-setup.js';

const missing = {state:'not_installed'};
const ready = {state:'ready'};
const progress = (state, extra={}) => ({state, downloadedBytes:40, totalBytes:100, canInstall:true, ...extra});

test('confirmed missing tools direct launch to setup; uncertain checks do not', () => {
  assert.equal(needsSetupOnLaunch(voiceSetupNotice(missing,progress('idle',{downloadedBytes:0}))),true);
  assert.equal(needsSetupOnLaunch(voiceSetupNotice(null,null)),false);
  assert.equal(needsSetupOnLaunch(voiceSetupNotice({state:'unknown'},null)),false);
});

test('ready legacy installs do not advertise unfinished setup or redirect', () => {
  const notice=voiceSetupNotice(ready,progress('idle',{canInstall:false,downloadedBytes:0}));
  assert.equal(notice.visible,false);
  assert.equal(needsSetupOnLaunch(notice),false);
});

test('paused and failed upgrades remain visible even while existing tools are usable', () => {
  for(const [state,action] of [['paused','Continue setup'],['failed','Retry setup']]){
    const notice=voiceSetupNotice(ready,progress(state));
    assert.equal(notice.toolsReady,true);
    assert.equal(notice.visible,true);
    assert.equal(notice.action,action);
    assert.equal(needsSetupOnLaunch(notice),true);
    assert.equal(notice.percent,40);
  }
});

test('active setup shows progress and completion removes the shortcut', () => {
  const notice=voiceSetupNotice(missing,progress('checking',{downloadedBytes:100}));
  assert.equal(notice.phase,'active');
  assert.equal(notice.percent,100);
  assert.equal(notice.working,true);
  assert.equal(notice.action,'View setup');
  assert.equal(voiceSetupNotice(ready,progress('completed',{downloadedBytes:100})).visible,false);
});

test('a stale installed marker cannot hide missing or damaged tools', () => {
  const notice=voiceSetupNotice(missing,progress('installed',{downloadedBytes:100}));
  assert.equal(notice.visible,true);
  assert.equal(notice.toolsReady,false);
});

test('unavailable releases explain their status without suggesting a download can start', () => {
  const notice=voiceSetupNotice(missing,progress('idle',{downloadedBytes:0,canInstall:false,stage:'Compatible resources are not published.'}));
  assert.equal(notice.action,'View setup');
  assert.equal(notice.description,'Compatible resources are not published.');
});


test('recognition alone cannot complete the everyday voice setup', () => {
  const notice=voiceSetupNotice(ready,progress('idle',{downloadedBytes:0,ready:false}));
  assert.equal(notice.visible,true);
  assert.equal(notice.toolsReady,false);
  assert.equal(needsSetupOnLaunch(notice),true);
  assert.equal(voiceSetupNotice(missing,progress('installed',{ready:true})).visible,false);
});
