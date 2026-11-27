/** @typedef {{state:string,message?:string}|null} ToolStatus */
/** @typedef {{state:string,stage?:string,downloadedBytes:number,totalBytes:number,canInstall:boolean,ready?:boolean,error?:string|null}|null} SetupProgress */
/** @param {ToolStatus} status @param {SetupProgress} progress */
export function voiceSetupNotice(status, progress) {
  const active = ['preparing','downloading','installing','checking','activating'].includes(progress?.state ?? '');
  const toolsReady = typeof progress?.ready === 'boolean' ? progress.ready : status?.state === 'ready' || (!status && ['installed','completed'].includes(progress?.state ?? ''));
  const downloadedBytes = Math.max(0, progress?.downloadedBytes ?? 0);
  const totalBytes = Math.max(0, progress?.totalBytes ?? 0);
  const percent = totalBytes > 0 ? Math.min(100, Math.floor(downloadedBytes / totalBytes * 100)) : null;
  let phase = 'checking', title = 'Checking your voice tools…', description = 'Your setup status will appear here.', action = 'View setup';
  if (active) {
    phase = 'active'; title = progress?.state === 'downloading' ? 'Your voice tools are downloading' : progress?.state==='preparing'?'Setting up your voice tools':'Finishing your voice setup';
    description = progress?.stage || 'You can continue browsing while setup runs.';
  } else if (progress?.state === 'failed' || progress?.error) {
    phase = 'failed'; title = 'Your voice setup needs attention';
    description = progress?.error || 'Your saved download can be retried. Your recordings are safe.'; action = 'Retry setup';
  } else if (progress?.state === 'paused' || (!toolsReady && downloadedBytes > 0)) {
    phase = 'paused'; title = 'Continue setting up your voice tools';
    description = 'Your download is saved. Continue from where you left off.'; action = 'Continue setup';
  } else if (toolsReady) {
    phase = 'ready'; title = 'Voice tools are ready'; description = 'Ready to work offline.';
  } else if (progress?.ready===false || status && status.state !== 'unknown') {
    phase = 'missing'; title = 'Set up your voice tools';
    description = 'Set up transcription, dictation and Text to Speech once, then work offline.'; action = 'Set up voice tools';
    if (progress && !progress.canInstall) {description=progress.stage || 'Open setup to check which tools are available for this computer.';action='View setup';}
  }
  return {visible: phase !== 'ready', toolsReady, working:active&&progress?.state!=='downloading', phase, title, description, action, downloadedBytes, totalBytes, percent};
}

/** Only initial, confirmed unfinished setup should change the launch destination.
 * @param {ReturnType<typeof voiceSetupNotice>} notice */
export function needsSetupOnLaunch(notice) {
  return ['missing', 'paused', 'failed', 'active'].includes(notice.phase);
}
