/** @typedef {{temperature?:boolean,sentencePause?:boolean,speed?:boolean,duration?:boolean,numSteps?:boolean,seed?:boolean,temperatureMin?:number,defaultTemperature?:number|null,defaultSentencePause?:number|null,defaultSpeed?:number,defaultNumSteps?:number|null}} SpeechCapabilities */
/** @typedef {{temperature:number|undefined,sentencePause:number|undefined,speed:number|undefined,duration:number|undefined,useDuration:boolean,numSteps:number|undefined,automaticSeed:boolean,seed:number|undefined}} SpeechControlValues */
/** @typedef {{schemaVersion:1,seed:number,temperature?:number,sentencePause?:number,speed?:number,duration?:number,numSteps?:number}} AppliedSpeechSettings */
/** @param {{provider?:string|null,controls?:SpeechCapabilities}|null} status @returns {SpeechCapabilities} */
export function speechCapabilities(status){
 const small=status?.provider==='tts-flow_lm-90m-amh-fp32',omni=status?.provider?.startsWith('tts-codec_llm-0.6b-amh-')??false,known=small||omni;
 return {temperature:known,sentencePause:known,speed:omni,duration:omni,numSteps:omni,seed:true,temperatureMin:small?.1:0,defaultTemperature:small?.3:0,defaultSentencePause:.2,defaultSpeed:1,defaultNumSteps:32,...status?.controls,...(status?.controls?{speed:!!status.controls.speed,duration:!!status.controls.duration,numSteps:!!status.controls.numSteps}:{})};
}
/** @param {SpeechCapabilities} c @returns {SpeechControlValues} */
export function defaultSpeechControls(c){return {temperature:c.defaultTemperature??.3,sentencePause:c.defaultSentencePause??.2,speed:c.defaultSpeed??1,duration:5,useDuration:false,numSteps:c.defaultNumSteps??32,automaticSeed:true,seed:0};}
/** @param {unknown} value @param {number} low @param {number} high */
function inRange(value,low,high){return typeof value==='number'&&Number.isFinite(value)&&value>=low&&value<=high;}
/** @param {SpeechControlValues} v @param {SpeechCapabilities} c */
export function speechControlError(v,c){
 if(c.temperature&&!inRange(v.temperature,c.temperatureMin??.1,1))return `Choose a temperature between ${c.temperatureMin??.1} and 1.0.`;
 const fixed=c.duration&&v.useDuration;
 if(fixed&&!inRange(v.duration,.5,600))return 'Choose a target duration between 0.5 and 600 seconds.';
 if(!fixed&&c.speed&&!inRange(v.speed,.5,2))return 'Choose a speaking speed between 0.5× and 2×.';
 if(!fixed&&c.sentencePause&&!inRange(v.sentencePause,0,2))return 'Choose a sentence pause between 0 and 2 seconds.';
 if(c.numSteps&&![16,32,64].includes(v.numSteps??0))return 'Choose 16, 32 or 64 generation steps.';
 if(!v.automaticSeed&&(!Number.isInteger(v.seed)||!inRange(v.seed,0,4294967295)))return 'Choose a whole-number seed from 0 to 4294967295.';
 return '';
}
/** @param {SpeechControlValues} values @param {SpeechCapabilities} capabilities @param {()=>number} [randomSeed] */
export function speechGenerationRequest(values,capabilities,randomSeed=()=>crypto.getRandomValues(new Uint32Array(1))[0]){
 const problem=speechControlError(values,capabilities);if(problem)throw Error(problem);
 const seed=values.automaticSeed?randomSeed():/** @type {number} */(values.seed);
 if(!Number.isInteger(seed)||!inRange(seed,0,4294967295))throw Error('Could not create a variation seed. Try again.');
 /** @type {{temperature?:number,sentencePause?:number,speed?:number,duration?:number,numSteps?:number}} */const options={};
 if(capabilities.temperature)options.temperature=values.temperature;
 if(capabilities.numSteps)options.numSteps=values.numSteps;
 if(capabilities.duration&&values.useDuration)options.duration=values.duration;
 else{if(capabilities.speed)options.speed=values.speed;if(capabilities.sentencePause)options.sentencePause=values.sentencePause;}
 return {seed,generationOptions:Object.keys(options).length?options:null};
}
/** @param {unknown} value @returns {AppliedSpeechSettings|null} */
export function savedSpeechSettings(value){
 const s=/** @type {Partial<AppliedSpeechSettings>|null} */(value);
 if(!s||s.schemaVersion!==1||!Number.isInteger(s.seed)||!inRange(s.seed,0,4294967295))return null;
 /** @type {AppliedSpeechSettings} */const result={schemaVersion:1,seed:/** @type {number} */(s.seed)};
 if(inRange(s.temperature,0,1))result.temperature=s.temperature;
 if(inRange(s.duration,.5,600))result.duration=s.duration;
 else{if(inRange(s.sentencePause,0,2))result.sentencePause=s.sentencePause;if(inRange(s.speed,.5,2))result.speed=s.speed;}
 if([16,32,64].includes(s.numSteps??0))result.numSteps=s.numSteps;
 return result;
}
