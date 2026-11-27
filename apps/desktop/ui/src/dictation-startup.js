/** @typedef {'audio'|'voice-tools'|'microphone'|'capture'} StartupPhase */
/** Native startup errors are controlled messages, never inference text. */
/** @param {unknown} cause @param {StartupPhase} phase */
export function dictationStartupMessage(cause,phase){
  const e=/** @type {{name?:string;message?:string;constraint?:string}|null} */(cause);
  const name=e&&typeof e==='object'?e.name:'';
  if(name==='NotAllowedError'||name==='PermissionDeniedError')return 'Microphone access was not allowed. Enable Ethiopic Voice Studio in macOS Settings → Privacy & Security → Microphone, then try again.';
  if(name==='NotFoundError'||name==='DevicesNotFoundError')return 'No microphone was found. Connect one or choose another microphone in Settings.';
  if(name==='OverconstrainedError'||name==='ConstraintNotSatisfiedError')return 'The selected microphone is unavailable. Choose another microphone in Settings.';
  if(name==='NotReadableError'||name==='TrackStartError')return 'The microphone could not be opened. Check your input device and close other recording apps, then try again.';
  if(phase==='voice-tools'){
    const message=typeof cause==='string'?cause:e?.message;
    if(typeof message==='string'&&message.trim()&&message.length<=400&&!/[\r\n/\\]/.test(message))return message;
    return 'Voice tools could not start. Open Settings to check the installed voice tools, then try again.';
  }
  if(phase==='capture')return 'The microphone opened, but audio capture could not start. Restart Studio and try again.';
  if(phase==='audio')return 'Audio could not start. Check the computer’s input and output devices, then try again.';
  return 'The microphone could not start. Choose a microphone in Settings and try again.';
}
