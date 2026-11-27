import test from 'node:test';
import assert from 'node:assert/strict';
import {existsSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
import {preview} from 'vite';
import {chromium} from 'playwright-core';

const executable=process.env.CHROME_PATH||'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
test('bug reporting opens the web form and copies only app details',{skip:!existsSync(executable)},async()=>{
  const server=await preview({root:fileURLToPath(new URL('..',import.meta.url)),preview:{host:'127.0.0.1',port:0,open:false}});
  const browser=await chromium.launch({executablePath:executable,headless:true});
  try{
    const page=await browser.newPage({viewport:{width:1200,height:1000},colorScheme:'dark'});
    const errors=[];page.on('pageerror',error=>errors.push(error.message));
    await page.addInitScript(()=>{
      window.reportCalls=[];window.failOpen=false;window.failCopy=false;window.copiedReport='';
      window.reportBody=JSON.stringify({schema_version:1,version:'0.1.0-rc.2',os:'macos',os_version:'15.0',arch:'aarch64',model_catalog_revision:'a8d70e01',tools:[{tool:'dictation',model_id:'asr-wav2vec_ctc-1b-amh-int8_onnx',model_version:'v0.3',backend:'onnx',device:'cpu',state:'installed'}]},null,2);
      Object.defineProperty(navigator,'platform',{value:'MacIntel',configurable:true});
      Object.defineProperty(navigator,'clipboard',{value:{writeText:async text=>{if(window.failCopy)throw Error('Clipboard unavailable');window.copiedReport=text;}},configurable:true});
      window.__TAURI_INTERNALS__={invoke:async(command,args)=>{
        if(['open_bug_report','copy_bug_report','get_bug_report'].includes(command))window.reportCalls.push({command,args});
        if(command==='open_bug_report'){if(window.failOpen)throw 'No browser';return;}
        if(command==='copy_bug_report'){if(window.failCopy)throw 'Clipboard unavailable';window.copiedReport=window.reportBody;return;}
        if(command==='get_bug_report')return window.reportBody;
        if(command==='system_summary')return {os:'macos',arch:'aarch64',logicalCpus:8,totalMemoryBytes:16*1024**3};
        if(command==='get_asr_status')return {state:'ready',message:'Ready'};
        if(command==='get_active_dictation_session')return null;
        throw Error('Outside this test');
      }};
    });
    await page.goto(server.resolvedUrls.local[0]);
    await page.getByRole('button',{name:'Settings',exact:true}).click();
    const section=page.getByRole('region',{name:'Bug reports',exact:true});
    const open=section.getByRole('button',{name:'Report a bug',exact:true});
    const copy=section.getByRole('button',{name:'Copy app details',exact:true});
    await open.waitFor();
    assert.deepEqual(await page.evaluate(()=>window.reportCalls),[],'opening Settings must not launch the browser or collect report contents');
    assert.match(await section.innerText(),/online form/);
    await open.click();
    await section.getByRole('status').filter({hasText:'Complete the report in your browser'}).waitFor();
    assert.deepEqual(await page.evaluate(()=>window.reportCalls.map(call=>call.command)),['open_bug_report']);
    await page.evaluate(()=>window.failOpen=true);
    await open.click();
    await section.getByRole('alert').filter({hasText:'Could not open the form.'}).waitFor();
    await copy.click();
    await section.getByRole('status').filter({hasText:'App details copied.'}).waitFor();
    assert.equal(await page.evaluate(()=>window.copiedReport),await page.evaluate(()=>window.reportBody));
    assert.equal(await section.getByRole('alert').count(),0,'successful fallback clears the browser error');
    assert.equal(await page.evaluate(()=>window.reportCalls.filter(c=>c.command==='copy_bug_report').length),1);
    await page.evaluate(()=>Object.defineProperty(navigator,'platform',{value:'Win32',configurable:true}));
    await copy.click();
    await page.waitForFunction(()=>window.reportCalls.some(c=>c.command==='get_bug_report'));
    assert.equal(await page.evaluate(()=>window.copiedReport),await page.evaluate(()=>window.reportBody));
    await page.evaluate(()=>window.failCopy=true);
    await copy.click();
    await section.getByRole('alert').filter({hasText:'Could not copy app details.'}).waitFor();
    await page.setViewportSize({width:650,height:900});
    await section.scrollIntoViewIfNeeded();
    assert.equal(await section.evaluate(element=>element.scrollWidth<=element.clientWidth),true);
    await page.screenshot({path:'/tmp/ethiopic-bug-report-settings.png'});
    assert.deepEqual(errors,[]);
  }finally{
    await browser.close();
    await new Promise((resolve,reject)=>server.httpServer.close(error=>error?reject(error):resolve()));
  }
});
