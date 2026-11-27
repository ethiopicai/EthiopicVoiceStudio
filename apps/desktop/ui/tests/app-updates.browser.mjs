import test from 'node:test';
import assert from 'node:assert/strict';
import {existsSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
import {preview} from 'vite';
import {chromium} from 'playwright-core';
import {seedVoiceHistory} from './voice-history-fixture.mjs';
const executable=process.env.CHROME_PATH||'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
test('startup and foreground update checks, Home reminder, dismissal and independent opt-outs',{skip:!existsSync(executable),timeout:60000},async()=>{
 const server=await preview({root:fileURLToPath(new URL('..',import.meta.url)),preview:{host:'127.0.0.1',port:0,open:false}}),browser=await chromium.launch({executablePath:executable,headless:true});
 try{
  const page=await browser.newPage({viewport:{width:1300,height:1000}}),errors=[];page.on('pageerror',e=>errors.push(e.message));
  await page.addInitScript(seedVoiceHistory,0);
  await page.addInitScript(()=>{
   if(window.self!==window.top)return;
   const base=window.__TAURI_INTERNALS__.invoke;
   window.updateCalls=[];window.updateOffline=false;window.updateOpenCount=0;
   window.updateState={currentVersion:'0.1.0-rc.2',automaticChecks:true,usageStatistics:true,lastCheckedAt:null,lastSuccessfulCheckAt:null,checking:false,availableUpdate:null,showNotification:false,error:null};
   window.__TAURI_INTERNALS__.invoke=async(command,args={})=>{
    if(command==='get_voice_setup')return {state:'ready',ready:true};
    if(command==='get_app_update_status')return structuredClone(window.updateState);
    if(command==='check_app_update'){
     window.updateCalls.push(args.manual);window.updateState.lastCheckedAt=1795700000;
     if(window.updateOffline)window.updateState.error='Could not check for updates. Connect to the internet and try again.';
     else {window.updateState.lastSuccessfulCheckAt=1795700000;window.updateState.error=null;window.updateState.availableUpdate={version:'0.1.0-rc.3',notes:'New speech controls.\n<script>never execute</script>',url:'https://ethiopic.ai/studio/'};window.updateState.showNotification=!window.updateDismissed;}
     return structuredClone(window.updateState);
    }
    if(command==='set_app_update_preferences'){window.updateState.automaticChecks=args.automaticChecks;window.updateState.usageStatistics=args.usageStatistics;return structuredClone(window.updateState);}
    if(command==='dismiss_app_update'){window.updateDismissed=true;window.updateState.showNotification=false;return structuredClone(window.updateState);}
    if(command==='open_app_update'){window.updateOpenCount++;return;}
    return base(command,args);
   };
  });
  await page.goto(server.resolvedUrls.local[0]);
  const banner=page.getByRole('status',{name:'App update available',exact:true}),home=page.getByRole('status',{name:'Home app update',exact:true});
  await home.waitFor();assert.equal(await banner.count(),0);assert.ok((await page.evaluate(()=>window.updateCalls)).includes(false));
  const sidebar=page.locator('.sidebar-updates'),sidebarCheck=sidebar.getByRole('button',{name:'Check for updates',exact:true});
  assert.equal((await sidebar.locator('.app-version').innerText()).trim(),'v0.1.0-rc.2');
  const priorChecks=(await page.evaluate(()=>window.updateCalls)).length;await sidebarCheck.click();
  await page.waitForFunction(n=>window.updateCalls.length>n,priorChecks);assert.equal(await page.evaluate(()=>window.updateCalls.at(-1)),true);await home.waitFor();
  await page.setViewportSize({width:900,height:620});
  for(const collapsed of [false,true]){
   if(collapsed)await page.getByRole('button',{name:'Collapse navigation',exact:true}).click();
   const bounds=await sidebar.boundingBox();assert.ok(bounds.y>=0&&bounds.y+bounds.height<=620,'Version and update controls remain on screen');
   assert.ok(await sidebar.locator('.app-version').isVisible());assert.ok(await sidebarCheck.isVisible());
   assert.ok(await sidebar.locator('.app-version').evaluate(node=>node.scrollWidth<=node.clientWidth));
  }
  await page.screenshot({path:'/tmp/ethiopic-sidebar-version-collapsed.png'});
  await page.getByRole('button',{name:'Expand navigation',exact:true}).click();await page.setViewportSize({width:1300,height:1000});
  await page.screenshot({path:'/tmp/ethiopic-sidebar-version.png'});
  await page.locator('button.nav-item[aria-label="Text to Speech"]').click();await banner.waitFor();
  await banner.getByRole('button',{name:'Dismiss update notification'}).click();await banner.waitFor({state:'detached'});await page.getByRole('button',{name:'Home',exact:true}).click();await home.waitFor();
  await home.getByRole('button',{name:'View update'}).click();assert.equal(await page.evaluate(()=>window.updateOpenCount),1);
  await home.getByRole('button',{name:'Details'}).click();
  const settings=page.getByRole('region',{name:'App updates',exact:true});await settings.waitFor();
  assert.match(await settings.innerText(),/0.1.0-rc.3/);assert.match(await settings.innerText(),/<script>never execute<\/script>/);assert.equal(await settings.locator('script').count(),0);
  await settings.getByRole('checkbox',{name:'Share basic usage statistics'}).uncheck();
  assert.equal(await page.evaluate(()=>window.updateState.usageStatistics),false);
  assert.equal(await settings.getByRole('checkbox',{name:'Check automatically when online'}).isChecked(),true);
  await settings.getByRole('checkbox',{name:'Check automatically when online'}).uncheck();
  const calls=(await page.evaluate(()=>window.updateCalls)).length;
  await page.evaluate(()=>window.dispatchEvent(new Event('focus')));await page.waitForTimeout(150);assert.equal((await page.evaluate(()=>window.updateCalls)).length,calls);
  await settings.getByRole('button',{name:'Check now',exact:true}).click();await page.waitForFunction(()=>window.updateCalls.at(-1)===true);
  await page.evaluate(()=>window.updateOffline=true);await settings.getByRole('button',{name:'Check now',exact:true}).click();await settings.getByText('Could not check for updates. Connect to the internet and try again.').waitFor();
  assert.match(await settings.innerText(),/Last successful check/);assert.match(await settings.innerText(),/0.1.0-rc.3/);
  await page.screenshot({path:'/tmp/ethiopic-update-settings.png'});
  await page.getByRole('button',{name:'Home',exact:true}).click();await home.waitFor();await banner.waitFor({state:'detached'});
  await page.screenshot({path:'/tmp/ethiopic-home-update-note.png'});
  await page.locator('button.nav-item[aria-label="Text to Speech"]').click();const script=page.getByLabel('Script',{exact:true});await script.fill('Unsaved script stays here.');
  await page.getByRole('button',{name:'Settings',exact:true}).click();await settings.getByRole('checkbox',{name:'Check automatically when online'}).check();
  await page.locator('button.nav-item[aria-label="Text to Speech"]').click();await script.fill('Unsaved script stays here.');
  const before=(await page.evaluate(()=>window.updateCalls)).length;
  await page.evaluate(()=>{window.updateOffline=false;window.dispatchEvent(new Event('focus'));});await page.waitForFunction(n=>window.updateCalls.length>n,before);assert.equal(await script.inputValue(),'Unsaved script stays here.');
  assert.deepEqual(errors,[]);
 }finally{await browser.close();await new Promise(resolve=>server.httpServer.close(resolve));}
});
