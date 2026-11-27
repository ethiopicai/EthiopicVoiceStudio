import test from 'node:test';
import assert from 'node:assert/strict';
import {existsSync,mkdtempSync,rmSync} from 'node:fs';
import {tmpdir} from 'node:os';
import {join} from 'node:path';
import {fileURLToPath} from 'node:url';
import {createServer} from 'vite';
import {chromium} from 'playwright-core';
const executable=process.env.CHROME_PATH||'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';

test('standalone timed transcript follows an external player with no native model or project',{skip:!existsSync(executable),timeout:45000},async()=>{
  const cacheDir=mkdtempSync(join(tmpdir(),'ethiopic-timed-vite-'));
  const server=await createServer({root:fileURLToPath(new URL('..',import.meta.url)),cacheDir,server:{host:'127.0.0.1',port:0,strictPort:false,open:false}});await server.listen();
  const browser=await chromium.launch({executablePath:executable,headless:true});
  try{
    const page=await browser.newPage({viewport:{width:900,height:800}});
    await page.goto(server.resolvedUrls.local[0]+'tests/fixtures/timed-transcript.html');
    const primary=page.locator('#primary');await primary.getByText('<script>safe</script>',{exact:true}).waitFor();
    assert.equal(await page.evaluate(()=>typeof window.__TAURI_INTERNALS__),'undefined');
    await page.evaluate(()=>window.timedFixture.setClock(.2,true));
    assert.equal(await primary.getByRole('checkbox',{name:'Highlight words during playback',exact:true}).isChecked(),true);
    await primary.locator('[data-word-index="0"].playback-word').waitFor();
    assert.equal(await page.locator('#secondary .playback-word').count(),0);
    await page.evaluate(()=>window.timedFixture.setClock(.5,true));await primary.locator('.playback-word').waitFor({state:'detached'});
    await page.evaluate(()=>window.timedFixture.setClock(.7,true));await primary.locator('[data-word-index="1"].playback-word').waitFor();
    await page.evaluate(()=>window.timedFixture.setClock(.7,false));await primary.locator('.playback-word').waitFor({state:'detached'});
    await page.evaluate(()=>window.timedFixture.setClock(.1,true));await primary.locator('[data-word-index="0"].playback-word').waitFor();
    await page.evaluate(()=>window.timedFixture.setEnabled(false));await primary.locator('.playback-word').waitFor({state:'detached'});
    await page.evaluate(()=>window.timedFixture.setEnabled(true));await primary.locator('[data-word-index="0"].playback-word').waitFor();
    await primary.getByRole('button',{name:'ሰላም',exact:true}).focus();await page.keyboard.press('Enter');assert.equal(await page.evaluate(()=>window.timedFixture.selection()),1);
    await primary.getByRole('checkbox').uncheck();assert.equal(await primary.locator('.playback-word').count(),0);
    assert.equal(await page.locator('#secondary button.transcript-word').count(),0);
    await primary.getByRole('checkbox').check();
    await page.evaluate(()=>{window.timedFixture.replace([{text:'replacement',start:0,end:2}]);window.timedFixture.setClock(.8,true);});
    await primary.getByText('replacement',{exact:true}).waitFor();await primary.locator('[data-word-index="0"].playback-word').waitFor();
    await page.evaluate(()=>window.timedFixture.replace([{text:'bad order',start:1,end:2},{text:'earlier',start:0,end:1}]));await primary.locator('.playback-word').waitFor({state:'detached'});
    await page.evaluate(()=>window.timedFixture.replace([]));await primary.getByText('No timestamps',{exact:true}).waitFor();
    await page.locator('#clock-focus').focus();const top=await page.evaluate(()=>window.scrollY);
    await page.evaluate(()=>window.timedFixture.followLast());
    await page.waitForFunction(()=>document.querySelector('#long [role="region"]').scrollTop>0);
    assert.equal(await page.evaluate(()=>document.activeElement.id),'clock-focus');assert.equal(await page.evaluate(()=>window.scrollY),top);
  }finally{await browser.close();await server.close();rmSync(cacheDir,{recursive:true,force:true});}
});
