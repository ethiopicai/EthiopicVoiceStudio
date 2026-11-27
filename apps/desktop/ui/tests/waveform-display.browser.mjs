import test from 'node:test';
import assert from 'node:assert/strict';
import {existsSync,mkdtempSync,rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {fileURLToPath} from 'node:url';
import {createServer} from 'vite';
import {chromium} from 'playwright-core';
const executable=process.env.CHROME_PATH||'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';

test('quiet waveforms fill the display range while silence, original WAV bytes and decoded playback amplitudes stay unchanged',{skip:!existsSync(executable)},async()=>{
  const cacheDir=mkdtempSync(join(tmpdir(),'ethiopic-waveform-vite-'));
  const server=await createServer({root:fileURLToPath(new URL('..',import.meta.url)),cacheDir,server:{host:'127.0.0.1',port:0,strictPort:false,open:false}});await server.listen();
  const browser=await chromium.launch({executablePath:executable,headless:true,args:['--mute-audio']});
  try{
    const page=await browser.newPage({viewport:{width:1200,height:500},colorScheme:'dark'});
    const errors=[];page.on('pageerror',error=>errors.push(error.message));
    await page.goto(`${server.resolvedUrls.local[0]}tests/fixtures/waveform-display.html`);
    const bars=page.locator('#quiet .waveform-track line:not(.playhead)');
    await bars.first().waitFor({state:'attached'});await page.locator('#silent .waveform-track line:not(.playhead)').first().waitFor({state:'attached'});
    assert.equal(await page.locator('#quiet .waveform-track svg').isVisible(),true);
    const displayPeak=await bars.evaluateAll(lines=>Math.max(...lines.flatMap(line=>[Math.abs(24-Number(line.getAttribute('y1'))),Math.abs(24-Number(line.getAttribute('y2')))]))/23);
    assert.ok(Math.abs(displayPeak-.99)<.000001,`quiet display peak should be 0.99, got ${displayPeak}`);
    assert.equal(await page.locator('#silent .waveform-track line:not(.playhead)').evaluateAll(lines=>lines.every(line=>line.getAttribute('y1')==='24'&&line.getAttribute('y2')==='24')),true,'silence stays flat');
    assert.equal(await bars.evaluateAll(lines=>lines.some(line=>line.getAttribute('y1')==='24'&&line.getAttribute('y2')==='24')),true,'silent portions of quiet audio also keep their baseline');
    const sourceUnchanged=await page.evaluate(async()=>{
      const actual=new Uint8Array(await (await fetch(document.querySelector('#quiet audio').src)).arrayBuffer());
      return actual.length===window.waveformFixture.quiet.length&&actual.every((byte,i)=>byte===window.waveformFixture.quiet[i]);
    });
    assert.equal(sourceUnchanged,true,'media playback still uses the original WAV');
    const originalPeak=await page.evaluate(()=>Math.max(...window.waveformFixture.decoded.map(item=>item.snapshot.reduce((peak,sample)=>Math.max(peak,Math.abs(sample)),0))));
    assert.ok(originalPeak>0&&originalPeak<.003,'fixture audio must remain quiet');
    await page.getByRole('button',{name:'Play quiet word span',exact:true}).click();
    await page.waitForFunction(()=>window.waveformFixture.wordPeak!==null);
    assert.equal(await page.evaluate(()=>window.waveformFixture.wordPeak),originalPeak,'word playback uses original amplitudes');
    assert.equal(await page.evaluate(()=>window.waveformFixture.decoded.every(item=>item.snapshot.every((sample,i)=>sample===item.buffer.getChannelData(0)[i]))),true,'display scaling never modifies decoded PCM');
    assert.equal(await page.locator('#quiet audio').evaluate(audio=>audio.volume),1,'display scaling does not change playback volume');
    assert.equal(await page.evaluate(()=>window.waveformFixture.error),null);
    await page.screenshot({path:'/tmp/ethiopic-waveform-normalized.png'});
    assert.deepEqual(errors,[]);
  }finally{await browser.close();await server.close();rmSync(cacheDir,{recursive:true,force:true});}
});
