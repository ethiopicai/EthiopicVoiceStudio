import rules from './speech-text-rules.json' with {type:'json'};
import {expandAbbreviations} from './speech-abbreviations.js';
const {ones,tens}=rules;
const DECIMAL=/\p{Nd}/u;
/** @param {string} text */
function decimalDigits(text){return text.replace(/\p{Nd}/gu,c=>{const cp=c.codePointAt(0)??0;let first=cp;while(first>0&&DECIMAL.test(String.fromCodePoint(first-1)))first--;return String((cp-first)%10);});}
/** @param {bigint} n @returns {string} */
function integer(n){
 if(n<10n)return ones[Number(n)];if(n<20n)return n===10n?'አስር':`አስራ ${ones[Number(n-10n)]}`;
 if(n<100n)return tens[Number(n/10n)]+(n%10n?' '+ones[Number(n%10n)]:'');
 for(const [size,label] of [[1_000_000_000_000n,'ትሪሊዮን'],[1_000_000_000n,'ቢሊዮን'],[1_000_000n,'ሚሊዮን'],[1000n,'ሺህ'],[100n,'መቶ']]){
  const scale=/** @type {bigint} */(size);if(n>=scale)return `${integer(n/scale)} ${label}${n%scale?' '+integer(n%scale):''}`;
 }
 throw Error('Invalid number');
}
/** @param {string} text @returns {bigint|null} */
function ethiopicValue(text){
 /** @param {string} part @returns {bigint|null} */
 function small(part){if(!part)return 0n;if(part.length>2)return null;let total=0n,last=100;for(const c of part){const cp=c.charCodeAt(0),value=cp<=0x1371?cp-0x1368:(cp-0x1371)*10;if(value>=last||value<1||part.length===2&&total>0n&&(last<10||value>=10))return null;total+=BigInt(value);last=value;}return total;}
 /** @param {string} part @returns {bigint|null} */
 function group(part){const parts=part.split('፻');if(parts.length>2)return null;const high=small(parts[0]),low=parts.length===2?small(parts[1]):0n;if(high===null||low===null)return null;return parts.length===2?(high||1n)*100n+low:high;}
 let total=0n;const parts=text.split('፼');for(let i=0;i<parts.length;i++){const value=group(parts[i]);if(value===null)return null;total+=value;if(i<parts.length-1)total=(total||1n)*10000n;if(total>999_999_999_999_999n)return null;}return total||null;
}
/** @param {string} digits */
function eachDigit(digits){return Array.from(digits,c=>ones[Number(c)]).join(' ');}
/** @param {string} text @returns {{text:string,issues:string[]}} */
function numbers(text){
 const issues=[];let output=decimalDigits(text).replaceAll('٬',',').replaceAll('٫','.');
 output=output.replace(/[፩-፼]+/gu,value=>{const n=ethiopicValue(value);if(n===null){issues.push('Check the Ethiopic number and write it in words.');return value;}return n.toString();});
 if(/[+−-]\s+\d|[–—]\s*\d/u.test(output))issues.push('Write signed numbers without gaps, or write them in words.');
 if(/\d\.\d+,\d|\d,,\d|\d_\d/u.test(output))issues.push('Use commas only for thousands and a dot for decimals, or write the number in words.');
 if(/\d(?:\.\d+)?[eE][+-]?\d/u.test(output))issues.push('Write scientific notation in words.');
 if(/\d+\s*(?:ኛ|st\b|nd\b|rd\b|th\b)/iu.test(output))issues.push('Write ordinal numbers in words.');
 if(/\d\s*[-−–—/:⁄]\s*\d/u.test(output)||/\d\.\d+\./u.test(output))issues.push('Write dates, times, fractions and ranges in words so their meaning is clear.');
 output=output.replace(/[+−-]?(?:[0-9]+(?:,[0-9]+)*(?:\.[0-9]+)?|\.[0-9]+)(?:\s*[%٪])?/g,value=>{
  const negative=/^[-−]/.test(value),positive=value.startsWith('+'),percent=/[%٪]$/.test(value),unsigned=value.replace(/^[+−-]/,'').replace(/[%٪]$/,'').trim();
  if(unsigned.includes(',')&&!/^\d{1,3}(?:,\d{3})+(?:\.\d+)?$/.test(unsigned)){issues.push('Use commas only for thousands, or write this number in words.');return value;}
  const [whole,fraction]=unsigned.replaceAll(',','').split('.'),digits=whole||'0';
  const spoken=digits.length>15||digits.length>1&&digits.startsWith('0')?eachDigit(digits):integer(BigInt(digits));
  return ` ${negative?rules.minus+' ':positive?rules.plus+' ':''}${spoken}${fraction?' '+rules.point+' '+eachDigit(fraction):''}${percent?' '+rules.percent:''} `;
 });
 if(/\p{N}/u.test(output))issues.push('Write the remaining numeric symbols in words.');
 return {text:output.replace(/\s+/gu,' ').replace(/\s+([.,!?።፣፤፥፦፧])/gu,'$1').trim(),issues:[...new Set(issues)]};
}
/** Compatibility helper for the explicit editing action. @param {string} text */
export function normalizeNumbers(text){return numbers(text).text;}
/** Pure, offline preparation for text that will be synthesized, never reference transcripts.
 * @param {string} originalText @param {{allowEmpty?:boolean}} [options] */
export function prepareSpeechText(originalText,{allowEmpty=false}={}){
 const source=originalText.normalize('NFC').replace(/[\u0000-\u0008\u000b\u000c\u000e-\u001f\u007f\u200c-\u200f\u202a-\u202e\u2060-\u206f\ufeff]/gu,'').replace(/\u200b/gu,' ');
 const expanded=expandAbbreviations(source),converted=numbers(expanded.text),fold=/** @type {Record<string,string>} */(rules.fold),text=Array.from(converted.text,c=>fold[c]??c).join('');
 const issues=[...converted.issues],warnings=[];
 if(expanded.unknown.length)issues.push('Write unfamiliar or incomplete abbreviations in full: '+expanded.unknown.join(', '));
 else if(expanded.text.includes('/'))issues.push('Write abbreviations or slash-separated phrases in full.');
 if(!text&&(!allowEmpty||originalText.trim())||text&&!/\p{L}/u.test(text))issues.push('Enter words for the voice to read.');
 if(Array.from(originalText).length>12000||Array.from(text).length>12000)issues.push('Use up to 12,000 characters after expanding numbers and abbreviations. Split this script into parts.');
 if(/(?:https?:\/\/|www\.|\S+@\S+)/iu.test(source))issues.push('Write web and email addresses as you want them spoken.');
 if(/\p{Extended_Pictographic}/u.test(text))issues.push('Remove emoji or write their meaning in words.');
 if(/\p{Sm}/u.test(text))issues.push('Write equations and mathematical symbols in words.');
 if(/[$€£¥₹%٪]/u.test(text))issues.push('Write currency and other numeric symbols in words.');
 if(/\p{Script=Latin}/u.test(text))warnings.push('This voice is trained for Amharic. English words and names may be pronounced poorly; review their wording.');
 return {text,originalText,changed:text!==originalText,hasNumbers:/\p{N}/u.test(originalText),hasAbbreviations:expanded.changed,issues:[...new Set(issues)],warnings,version:rules.version};
}
