/** @typedef {{modifiers:number,key:string|null}} Shortcut */
/** @param {Shortcut} value @param {string} platform */
export function shortcutLabel(value,platform){
  const parts=[];
  for(const [flag,label] of [[1,'Ctrl'],[2,platform==='macos'?'Option':'Alt'],[4,'Shift'],[8,platform==='macos'?'Command':'Win']])if(value.modifiers&Number(flag))parts.push(label);
  if(value.key)parts.push(value.key.replace(/^(Key|Digit)/,''));
  return parts.join(' + ');
}
/** @param {Pick<KeyboardEvent,'code'|'ctrlKey'|'altKey'|'shiftKey'|'metaKey'|'repeat'|'isComposing'>} event @returns {Shortcut|null} */
export function shortcutFromKey(event){
  if(event.repeat||event.isComposing)return null;
  const modifiers=(event.ctrlKey?1:0)|(event.altKey?2:0)|(event.shiftKey?4:0)|(event.metaKey?8:0);
  if(/^(Control|Alt|Shift|Meta)(Left|Right)$/.test(event.code))return {modifiers,key:null};
  if(/^(Key[A-Z]|Digit[0-9]|Space|F([1-9]|1[0-2]))$/.test(event.code))return {modifiers,key:event.code};
  return null;
}
/** Native validation is authoritative. @param {Shortcut|null} shortcut @param {string} platform */
export function canSaveShortcut(shortcut,platform){
  if(!shortcut)return false;
  if(shortcut.key==='Fn')return platform==='macos'&&shortcut.modifiers===0;
  if(shortcut.key===null)return shortcut.modifiers.toString(2).replaceAll('0','').length>=2;
  return !!(shortcut.modifiers&(1|2|8));
}
