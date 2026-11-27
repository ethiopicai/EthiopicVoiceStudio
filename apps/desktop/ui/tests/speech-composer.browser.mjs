import test from 'node:test';
import assert from 'node:assert/strict';
import {existsSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
import {preview} from 'vite';
import {chromium} from 'playwright-core';
import {seedVoiceHistory} from './voice-history-fixture.mjs';
const executable=process.env.CHROME_PATH||'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';

test('speech creation stays distinct from reference management and browsing never replaces the current script',{skip:!existsSync(executable),timeout:60000},async()=>{
  const server=await preview({root:fileURLToPath(new URL('..',import.meta.url)),preview:{host:'127.0.0.1',port:0,open:false}});
  const browser=await chromium.launch({executablePath:executable,headless:true});
  try{
    const page=await browser.newPage({viewport:{width:1400,height:1000},colorScheme:'dark'}),errors=[];page.on('pageerror',error=>errors.push(error.message));
    await page.addInitScript(seedVoiceHistory,0);
    await page.addInitScript(()=>{if(window.self!==window.top)return;const base=window.__TAURI_INTERNALS__.invoke;window.__TAURI_INTERNALS__.invoke=async(command,args)=>command==='get_voice_generation_status'?{ready:true,consent:true,busy:false,requiresReferenceText:false,defaultVoice:true,reason:'Ready'}:base(command,args);});
    await page.goto(server.resolvedUrls.local[0]);await page.locator('button.nav-item[aria-label="Text to Speech"]').click();
    const composer=page.getByRole('region',{name:'Speech script',exact:true}),script=composer.getByRole('textbox',{name:'Script',exact:true}),library=page.locator('#voice-library');
    await composer.getByRole('heading',{name:'Synthesize speech',exact:true}).waitFor();
    assert.equal(await page.locator('button.nav-item[aria-label="Voice Clone"]').count(),0,'voice cloning is part of choosing a voice');
    assert.equal(await script.isVisible(),true);assert.equal(await library.evaluate(node=>node.open),false);
    assert.equal(await composer.locator('[data-workspace-card],.reference-card-body,.waveform-player').count(),0,'source players and results are outside the writing surface');
    assert.equal(await page.getByRole('region',{name:'Generated audio',exact:true}).locator('[data-workspace-card]').count(),1);
    const draft='ይህ አዲስ የፖድካስት ጽሑፍ ነው።';await script.fill(draft);
    const choice=composer.getByRole('combobox',{name:'Choose a voice',exact:true});
    assert.equal(await choice.locator('option[value="import"]').textContent(),'Import a new voice…');
    const chooserOpened=page.waitForEvent('filechooser');await choice.selectOption('import');const chooser=await chooserOpened;await chooser.setFiles([]);
    assert.equal(await choice.inputValue(),'preset:default','cancelling import preserves the chosen voice');assert.equal(await script.inputValue(),draft);
    const history=page.getByRole('region',{name:'Voice history',exact:true});
    await composer.getByRole('button',{name:'Manage voices',exact:true}).click();await library.getByRole('button',{name:'Open card: Voice one',exact:true}).click();
    await page.getByRole('region',{name:'Reference recording',exact:true}).waitFor();assert.equal(await library.evaluate(node=>node.open),true);
    assert.equal(await script.inputValue(),draft);
    await page.getByRole('button',{name:'Use this voice in script',exact:true}).click();
    assert.equal(await library.evaluate(node=>node.open),false);assert.equal(await script.evaluate(node=>document.activeElement===node),true);
    assert.equal(await script.inputValue(),draft);assert.equal(await composer.getByRole('combobox',{name:'Choose a voice',exact:true}).inputValue(),'profile:profile-one');
    await history.getByRole('button',{name:'Open generated speech: Generated one.wav',exact:true}).click();
    await page.getByRole('button',{name:'Edit & regenerate',exact:true}).waitFor();assert.equal(await script.inputValue(),draft,'browsing audio preserves unsaved work');
    await page.getByRole('button',{name:'Edit & regenerate',exact:true}).click();assert.equal(await script.inputValue(),'Saved speech');assert.equal(await page.locator('#voice-card-voice-project-generation-one #speech-composer').count(),1);
    await script.fill('Unsaved edit');await page.getByRole('button',{name:'Done editing',exact:true}).click();assert.equal(await script.inputValue(),draft,'finishing card editing restores the new speech draft');
    await history.getByRole('checkbox',{name:'Select generated speech: Generated one.wav',exact:true}).check();await history.getByRole('button',{name:'Delete selected',exact:true}).click();
    await page.getByRole('dialog').filter({visible:true}).getByRole('button',{name:'Delete from app',exact:true}).click();
    await history.getByRole('button',{name:'Open generated speech: Generated one.wav',exact:true}).waitFor({state:'detached'});assert.equal(await script.inputValue(),draft,'deleting output preserves the script');
    await page.screenshot({path:'/tmp/ethiopic-speech-composer-dark.png'});
    for(const [width,height] of [[1100,780],[820,740],[620,700]]){
      await page.setViewportSize({width,height});await page.getByRole('button',{name:'Hide voice history',exact:true}).click();
    await composer.scrollIntoViewIfNeeded();
      assert.equal(await page.locator('.voice-workspace').evaluate(node=>node.scrollWidth<=node.clientWidth+1),true,`workspace fits ${width}px`);
      assert.equal(await composer.evaluate(node=>node.scrollWidth<=node.clientWidth+1),true,`composer fits ${width}px`);
      await page.getByRole('button',{name:'Show voice history',exact:true}).click();
    }
    await page.setViewportSize({width:1100,height:780});await composer.scrollIntoViewIfNeeded();await page.screenshot({path:'/tmp/ethiopic-speech-composer-mac.png'});
    await page.evaluate(()=>document.documentElement.dataset.theme='light');await page.screenshot({path:'/tmp/ethiopic-speech-composer-light.png'});
    assert.deepEqual(errors,[]);
  }finally{await browser.close();await new Promise((resolve,reject)=>server.httpServer.close(error=>error?reject(error):resolve()));}
});
