import test from 'node:test';
import assert from 'node:assert/strict';
import { HOME_FRAME_SANDBOX, HOME_FRAME_ALLOW, HOME_FRAME_CSP, bundledHomeDocument, themedHomeDocument } from '../src/home-frame.js';
test('home document installs restrictive policy before bundled HTML', () => {
  const marker = '<style>body{color:green}</style><h1>ሰላም</h1>';
  const document = bundledHomeDocument(marker);
  assert.ok(document.startsWith('<!doctype html><html><head><meta http-equiv="Content-Security-Policy"'));
  assert.ok(document.indexOf(HOME_FRAME_CSP) < document.indexOf(marker));
  assert.ok(document.includes('content="no-referrer"'));
  assert.ok(document.includes(marker));
});
test('sandbox grants no active capability and permissions deny sensitive devices', () => {
  assert.equal(HOME_FRAME_SANDBOX, '');
  for (const capability of ['camera', 'microphone', 'geolocation', 'clipboard-read', 'clipboard-write']) assert.ok(HOME_FRAME_ALLOW.includes(`${capability} 'none'`));
});
test('frame CSP blocks scripts, network, nested frames, forms and base URL changes', () => {
  for (const directive of ['default-src', 'script-src', 'connect-src', 'media-src', 'frame-src', 'object-src', 'base-uri', 'form-action']) assert.ok(HOME_FRAME_CSP.includes(`${directive} 'none'`));
  assert.ok(!HOME_FRAME_CSP.includes('unsafe-eval'));
  assert.ok(HOME_FRAME_CSP.includes("style-src 'unsafe-inline'"));
});
test('bundled theme palette preserves the leading CSP and sandboxed home content',()=>{
  const original=bundledHomeDocument('<h1>Studio</h1>');
  const themed=themedHomeDocument(original,'light',':root{--studio-page:white}');
  assert.ok(themed.includes('<html data-theme="light">'));
  assert.ok(themed.includes(HOME_FRAME_CSP));
  assert.ok(themed.indexOf(HOME_FRAME_CSP)<themed.indexOf('studio-palette'));
  assert.ok(themed.endsWith('<body><h1>Studio</h1></body></html>'));
  assert.ok(!themed.includes('<script'));
});
