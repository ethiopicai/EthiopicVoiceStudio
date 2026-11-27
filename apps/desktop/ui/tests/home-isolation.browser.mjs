import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { chromium } from 'playwright-core';
import { HOME_FRAME_SANDBOX, HOME_FRAME_ALLOW, bundledHomeDocument } from '../src/home-frame.js';

const executable=process.env.CHROME_PATH||'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
const policy=JSON.parse(readFileSync(new URL('../../src-tauri/tauri.conf.json',import.meta.url),'utf8')).app.security.csp;

test('real sandbox blocks scripts, parent/bridge access, subresource network, forms, and top navigation', {skip:!existsSync(executable)}, async()=>{
  const browser=await chromium.launch({executablePath:executable,headless:true});
  try{
    const context=await browser.newContext();
    const requests=[];
    await context.route('**/*',route=>{requests.push(route.request().url());return route.abort();});
    const page=await context.newPage();
    // Use a real shell origin, like the desktop webview. Chrome crashes when
    // constructing this isolated srcdoc frame under an opaque about:blank host.
    await page.route('https://studio-shell.test/',route=>route.fulfill({contentType:'text/html',body:`<meta http-equiv="Content-Security-Policy" content="${policy}"><h1 id="host">Native shell</h1>`}));
    await page.goto('https://studio-shell.test/');
    await page.evaluate(()=>{window.bridgeCalls=0;window.__TAURI_INTERNALS__={invoke:()=>window.bridgeCalls++};});
    const raw=`<style>@import url('https://blocked.invalid/style.css'); p{color:rgb(55,211,156);background-image:url('https://blocked.invalid/pixel')}</style><script>parent.__TAURI_INTERNALS__.invoke('get_transcript');document.body.dataset.scriptRan='yes'</script><p id="safe">Readable HTML</p><img src="https://blocked.invalid/image" onerror="parent.bridgeCalls++"><form id="form" action="https://blocked.invalid/submit"><button>Submit</button></form><a id="escape" target="_top" href="https://blocked.invalid/escape">Escape</a>`;
    await page.evaluate(({html,sandbox,allow})=>{const frame=document.createElement('iframe');frame.title='Isolation test';frame.sandbox.value=sandbox;frame.allow=allow;frame.referrerPolicy='no-referrer';frame.srcdoc=html;document.body.append(frame);},{html:bundledHomeDocument(raw),sandbox:HOME_FRAME_SANDBOX,allow:HOME_FRAME_ALLOW});
    const frame=page.frameLocator('iframe');
    await frame.locator('#safe').waitFor();
    assert.equal(await frame.locator('#safe').evaluate(node=>getComputedStyle(node).color),'rgb(55, 211, 156)');
    assert.equal(await page.evaluate(()=>window.bridgeCalls),0);
    assert.equal(await frame.locator('body').getAttribute('data-script-ran'),null);
    assert.equal(await page.evaluate(()=>document.querySelector('iframe').contentDocument),null);
    assert.equal(await frame.locator('body').evaluate(()=>typeof window.__TAURI_INTERNALS__),'undefined');
    assert.equal(await frame.locator('body').evaluate(()=>{try{void parent.document;return false;}catch{return true;}}),true);
    await frame.locator('#escape').click();
    await frame.locator('#form button').click();
    // A following rendered frame gives queued navigation/resource requests a chance to run.
    await page.evaluate(()=>new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve))));
    assert.equal(page.url(),'https://studio-shell.test/');
    assert.deepEqual(requests,[]);
    assert.equal(context.pages().length,1);
  }finally{await browser.close();}
});
