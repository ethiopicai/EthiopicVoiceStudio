import speechRules from './speech-text-rules.json' with {type:'json'};
/** @param {string} text */
export function spokenToken(text){
  /** @type {Record<string,string>} */
  const fold=/** @type {Record<string,string>} */(speechRules.fold);
  return Array.from(text,c=>fold[c]??c).join('').replace(/[^\p{L}\p{N}]/gu,'');
}
/** @param {string} before @param {string} after */
export function speechChanged(before,after){return before.trim().split(/\s+/u).map(spokenToken).join(' ')!==after.trim().split(/\s+/u).map(spokenToken).join(' ');}
/** Explain every inactive editing action at the action itself.
 * @param {{selection:{available:boolean,opt_in_required?:boolean,reason:string},consent:boolean,ready:boolean,runtimePresent:boolean}|null} status
 * @param {{busy?:boolean,regenerate?:boolean,before?:string,after?:string,hasWords?:boolean}} options */
export function editingBlocker(status,{busy=false,regenerate=false,before='',after='',hasWords=true}={}){
  if(busy)return {code:'busy',message:'Wait for the current task to finish, or stop it before making another change.'};
  if(!status)return {code:'checking',message:'Checking whether Voice Editing is ready. If this continues, check voice tools again.'};
  if(status.selection.opt_in_required)return {code:'slow',message:status.selection.reason};
  if(!status.selection.available)return {code:'unavailable',message:status.selection.reason||'Speech regeneration is unavailable on this computer.'};
  if(!status.consent)return {code:'consent',message:'Read and accept the recording-rights note before editing speech.'};
  if(!status.ready)return {code:'setup',message:status.runtimePresent?'Prepare Voice Editing to download the required voice tools.':'Voice Editing needs its compatible runtime and models. Prepare Voice Editing to check setup.'};
  if(regenerate&&!speechChanged(before,after))return {code:'unchanged',message:'Change a spoken word in New wording to enable Regenerate. For punctuation or equivalent spelling, use Fix transcript.'};
  if(regenerate&&!hasWords)return {code:'alignment',message:'Locate the original words before regenerating. Align words first; your new wording will be kept.'};
  return null;
}
/** @param {string} text */
export function textWarnings(text){return {digits:/\p{Nd}/u.test(text),latin:/\p{Script=Latin}/u.test(text)};}
export {normalizeNumbers} from './speech-text.js';
/** Prefix/suffix trim plus bounded LCS avoids allocating a huge transcript matrix. */
/** @param {string} before @param {string} after */
export function wordDiff(before,after){
  const a=before.trim()?before.trim().split(/\s+/u):[],b=after.trim()?after.trim().split(/\s+/u):[];let left=0,right=0;
  while(left<a.length&&left<b.length&&a[left]===b[left])left++;
  while(right<a.length-left&&right<b.length-left&&a[a.length-1-right]===b[b.length-1-right])right++;
  const x=a.slice(left,a.length-right),y=b.slice(left,b.length-right);
  /** @type {{kind:string,text:string}[]} */ const result=[];
  /** @param {string} kind @param {string} text */
  const add=(kind,text)=>{if(!text)return;const last=result.at(-1);if(last?.kind===kind)last.text+=' '+text;else result.push({kind,text});};
  add('equal',a.slice(0,left).join(' '));
  if(x.length*y.length>250000){add('delete',x.join(' '));add('insert',y.join(' '));}
  else{const dp=Array.from({length:x.length+1},()=>new Uint32Array(y.length+1));
    for(let i=x.length-1;i>=0;i--)for(let j=y.length-1;j>=0;j--)dp[i][j]=x[i]===y[j]?dp[i+1][j+1]+1:Math.max(dp[i+1][j],dp[i][j+1]);
    let i=0,j=0;while(i<x.length||j<y.length){if(i<x.length&&j<y.length&&x[i]===y[j]){add('equal',x[i++]);j++;}else if(i<x.length&&(j===y.length||dp[i+1][j]>=dp[i][j+1]))add('delete',x[i++]);else add('insert',y[j++]);}}
  add('equal',right?a.slice(a.length-right).join(' '):'');return result;
}
