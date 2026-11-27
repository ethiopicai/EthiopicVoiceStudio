import {get,writable} from 'svelte/store';
export type FnGesture={requestId:number;pressed:boolean;cancelled:boolean};
export const fnGesture=writable<FnGesture|null>(null);
export function receiveFnGesture(value:FnGesture){
  if(!Number.isSafeInteger(value?.requestId)||value.requestId<1||typeof value.pressed!=='boolean'||typeof value.cancelled!=='boolean')return;
  const current=get(fnGesture);
  if(current&&value.requestId<current.requestId)return;
  fnGesture.set(value);
}
export function clearFnGesture(requestId:number){if(get(fnGesture)?.requestId===requestId)fnGesture.set(null);}
