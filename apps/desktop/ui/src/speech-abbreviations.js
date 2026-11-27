import rules from './speech-abbreviations.json' with {type:'json'};

// Compiled by the pinned Python AbbreviationExpander(), preserving its ordered
// replacements and repeated-slash support. No Python process per keystroke.
const patterns=rules.patterns.map(([pattern,meaning])=>({pattern:new RegExp(pattern,'gu'),meaning}));
/** @param {string} source */
export function expandAbbreviations(source){
 let text=source;
 for(const {pattern,meaning} of patterns)text=text.replace(pattern,()=>meaning);
 const unknown=[...new Set(text.match(/\p{L}+(?:(?:\s*\/+\s*|\.+)\p{L}+)+/gu)??[])].filter(value=>/\p{Script=Ethiopic}/u.test(value));
 return {text,changed:text!==source,unknown};
}
