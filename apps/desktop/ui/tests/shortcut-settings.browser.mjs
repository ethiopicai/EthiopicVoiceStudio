import test from 'node:test';
import assert from 'node:assert/strict';
import {existsSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
import {preview} from 'vite';
import {chromium} from 'playwright-core';
const executable=process.env.CHROME_PATH||'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
test('Mac and Windows shortcuts can be changed, saved, reset or cancelled without activating dictation',{skip:!existsSync(executable)},async()=>{
  const server=await preview({root:fileURLToPath(new URL('..',import.meta.url)),preview:{host:'127.0.0.1',port:0,open:false}});
  const browser=await chromium.launch({executablePath:executable,headless:true});
  try{
    for(const platform of ['macos','windows']){
      const page=await browser.newPage({viewport:{width:1200,height:1000}});
      await page.addInitScript(()=>{
        if(window.self!==window.top)return;
        window.platform=new URLSearchParams(location.search).get('testPlatform');
        window.capturing=false;window.failSave=false;window.modelStarts=0;
        window.defaultShortcut=window.platform==='windows'?{modifiers:9,key:null}:{modifiers:0,key:'Fn'};
        window.binding=JSON.parse(localStorage.getItem('fixture-shortcut')||'null')||window.defaultShortcut;
        const label=s=>[s.modifiers&1?'Ctrl':'',s.modifiers&2?(window.platform==='macos'?'Option':'Alt'):'',s.modifiers&4?'Shift':'',s.modifiers&8?(window.platform==='macos'?'Command':'Win'):'',s.key?.replace(/^(Key|Digit)/,'')].filter(Boolean).join(' + ');
        const status=()=>({supported:window.platform==='macos',platform:window.platform,enabled:false,requested:false,inputMonitoring:true,accessibility:true,active:null,shortcut:window.binding,shortcutLabel:label(window.binding),defaultShortcut:window.defaultShortcut});
        window.__TAURI_INTERNALS__={invoke:async(command,args)=>{
          if(command==='get_fn_shortcut_status')return status();
          if(command==='set_dictation_shortcut_capture'){window.capturing=args.capturing;return;}
          if(command==='set_dictation_shortcut'){
            if(window.failSave)throw 'Could not save shortcut settings';
            window.binding=args.shortcut;localStorage.setItem('fixture-shortcut',JSON.stringify(window.binding));window.capturing=false;return status();
          }
          if(command==='get_asr_status')return {state:'not_installed',message:'Needs setup'};
          if(command==='system_summary')return {os:window.platform,arch:'aarch64',logicalCpus:8};
          if(command==='get_fixture_progress')return {state:'idle',downloadedBytes:0,totalBytes:0};
          if(command==='start_dictation'){window.modelStarts++;throw 'not expected';}
          throw Error('Outside this test');
        }};
      });
      await page.goto(`${server.resolvedUrls.local[0]}?testPlatform=${platform}`);
      const settings=()=>page.getByRole('button',{name:'Settings',exact:true}).click();await settings();
      const row=page.locator('.shortcut-row');
      await row.getByText(platform==='windows'?'Ctrl + Win':'Fn',{exact:true}).waitFor();
      if(platform==='windows')assert.equal(await row.getByRole('button',{name:'Enable dictation shortcut',exact:true}).isDisabled(),true,'configuration must not pretend Windows activation is implemented');
      await row.getByRole('button',{name:'Change shortcut',exact:true}).click();
      assert.equal(await page.evaluate(()=>window.capturing),true);
      await page.keyboard.down('Control');
      assert.equal(await row.getByRole('button',{name:'Save shortcut',exact:true}).isDisabled(),true);
      await page.keyboard.down('Shift');await page.keyboard.press('d');await page.keyboard.up('Shift');await page.keyboard.up('Control');
      await row.getByRole('button',{name:'Save shortcut',exact:true}).click();
      assert.deepEqual(await page.evaluate(()=>window.binding),{modifiers:5,key:'KeyD'});
      assert.equal(await page.evaluate(()=>window.capturing),false);
      await page.reload();await settings();
      await row.getByText('Ctrl + Shift + D',{exact:true}).waitFor();
      await row.getByRole('button',{name:'Change shortcut',exact:true}).click();
      await page.keyboard.press('Escape');assert.equal(await page.evaluate(()=>window.capturing),false);
      await row.getByRole('button',{name:'Change shortcut',exact:true}).click();
      await page.keyboard.down('Control');await page.keyboard.press('e');await page.keyboard.up('Control');
      await page.evaluate(()=>window.failSave=true);await row.getByRole('button',{name:'Save shortcut',exact:true}).click();
      await row.getByRole('alert').filter({hasText:'Could not save shortcut settings'}).waitFor();
      assert.deepEqual(await page.evaluate(()=>window.binding),{modifiers:5,key:'KeyD'});
      await row.getByRole('button',{name:'Cancel',exact:true}).click();await page.evaluate(()=>window.failSave=false);
      await row.getByRole('button',{name:'Reset shortcut',exact:true}).click();
      await row.getByText(platform==='windows'?'Ctrl + Win':'Fn',{exact:true}).waitFor();
      await row.getByRole('button',{name:'Change shortcut',exact:true}).click();
      await page.evaluate(()=>window.dispatchEvent(new Event('blur')));assert.equal(await page.evaluate(()=>window.capturing),false);
      await row.getByRole('button',{name:'Change shortcut',exact:true}).click();
      await page.locator('button.nav-item[aria-label="Home"]').click();assert.equal(await page.evaluate(()=>window.capturing),false);
      assert.equal(await page.evaluate(()=>window.modelStarts),0);
      await page.close();
    }
  }finally{await browser.close();await new Promise((resolve,reject)=>server.httpServer.close(e=>e?reject(e):resolve()));}
});
