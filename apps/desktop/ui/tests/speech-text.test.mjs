import test from 'node:test';
import assert from 'node:assert/strict';
import {prepareSpeechText,normalizeNumbers} from '../src/speech-text.js';
test('speech text converts cardinal numbers, grouping, decimals, percentages and identifier zeros without losing precision',()=>{
 const examples=[
 ['12','አስራ ሁለት'],['105','አንድ መቶ አምስት'],['1,250.05','አንድ ሺህ ሁለት መቶ ሀምሳ ነጥብ ዜሮ አምስት'],
 ['001','ዜሮ ዜሮ አንድ'],['25 %','ሀያ አምስት በመቶ'],['-12','ቅንስናሽ አስራ ሁለት'],['1,000,000','አንድ ሚሊዮን'],
 ['.5','ዜሮ ነጥብ አምስት'],['ሰዎች 12።','ሰዎች አስራ ሁለት።'],['12 105','አስራ ሁለት አንድ መቶ አምስት']
 ];
 for(const [source,expected] of examples){const result=prepareSpeechText(source);assert.equal(result.text,expected,source);assert.deepEqual(result.issues,[],source);assert.equal(result.originalText,source);assert.equal(result.hasNumbers,true);assert.equal(result.version,'am-speech-v2');assert.deepEqual(prepareSpeechText(result.text).issues,[]);assert.equal(prepareSpeechText(result.text).text,result.text);}
 assert.equal(normalizeNumbers('10000000000000001').split(' ').length,17);
});
test('Unicode decimal digits and valid Ethiopic numerals become speakable Amharic',()=>{
 for(const text of ['١٢','۱۲','１２','𝟙𝟚','፲፪'])assert.equal(prepareSpeechText(text).text,'አስራ ሁለት',text);
 assert.equal(prepareSpeechText('፲፪%').text,'አስራ ሁለት በመቶ');assert.equal(prepareSpeechText('١٬٢٣٤٫٥').text,'አንድ ሺህ ሁለት መቶ ሰላሳ አራት ነጥብ አምስት');
 assert.equal(prepareSpeechText('፪፻፴፬').text,'ሁለት መቶ ሰላሳ አራት');assert.equal(prepareSpeechText('፻፼').text,'አንድ ሚሊዮን');
 for(const text of ['፩፪','፻፻','½','Ⅳ'])assert.ok(prepareSpeechText(text).issues.length,text);
});
test('normalization is idempotent, folds model spelling, removes invisible controls and preserves the submitted script',()=>{
 const original='  ሐይል\u200bሠላም\u200f\u0000\nዉ  ';
 const result=prepareSpeechText(original);assert.equal(result.text,'ሀይል ሰላም ው');assert.equal(result.originalText,original);assert.equal(result.changed,true);assert.equal(prepareSpeechText(result.text).changed,false);
 assert.equal(prepareSpeechText('Cafe\u0301').text,'Café');assert.equal(prepareSpeechText('12 Alex').warnings.length,1);
});
test('ambiguous formats require human wording rather than a guessed calendar or numeric meaning',()=>{
 for(const text of ['12/03/2026','፲፪/፫/፳፻፲፰','12:30','2-3','1,23','1.2.3','12ኛ','1.234,56','1_000','1,,000','- 12','1e6','12×3','👋','$12','25 % %','½','https://example.com/12','me@example.com'])assert.ok(prepareSpeechText(text).issues.length,text);
 assert.deepEqual(prepareSpeechText('12, 13').issues,[]);assert.equal(prepareSpeechText('12, 13').text,'አስራ ሁለት, አስራ ሶስት');
});
test('expanded text limits and intentional deletion are checked before any model call',()=>{
 assert.ok(prepareSpeechText('0 '.repeat(6000)).issues.length);assert.ok(prepareSpeechText('...').issues.length);assert.ok(prepareSpeechText('').issues.length);
 assert.deepEqual(prepareSpeechText('',{allowEmpty:true}).issues,[]);assert.ok(prepareSpeechText('\u200f',{allowEmpty:true}).issues.length);
});
test('upstream abbreviation expansion precedes number conversion and spelling folding',()=>{
 const input='ዶ/ር አበበ እና ኢ/ር ሐና 12 ሰዎች።';
 const result=prepareSpeechText(input);
 assert.equal(result.text,'ዶክተር አበበ እና ኢንጂነር ሀና አስራ ሁለት ሰዎች።');
 assert.equal(result.originalText,input);assert.equal(result.hasAbbreviations,true);assert.equal(result.hasNumbers,true);assert.deepEqual(result.issues,[]);
 for(const [source,expected] of [['ዶ.ር','ዶክተር'],['ኢ//ር','ኢንጂነር'],['ወ/ሮ','ወይዘሮ'],['ፕ/ር','ፕሮፌሰር'],['ት/ቤት','ትምህርት ቤት'],['ለዶ/ር','ለዶክተር'],['ለዶ.ር','ለዶክተር'],['ዓ/ም','አመተ ምህረት']]){
  const prepared=prepareSpeechText(source);assert.equal(prepared.text,expected,source);assert.deepEqual(prepared.issues,[],source);assert.equal(prepared.hasAbbreviations,true);assert.equal(prepareSpeechText(prepared.text).changed,false);
 }
 assert.ok(prepareSpeechText('ዶ/ር '.repeat(2500)).issues.some(issue=>issue.includes('12,000')));
 for(const source of ['ኡ/ኡ','ኡ.ኡ','ሀ/ለ','ሀ.ለ','ዶ/','ሰላም /'])assert.ok(prepareSpeechText(source).issues.some(issue=>issue.includes('abbreviations')),source);
 assert.equal(prepareSpeechText('ሰላም ጤና።').hasAbbreviations,false);
 assert.deepEqual(prepareSpeechText('ሰላም. ጤና.').issues,[]);
});
