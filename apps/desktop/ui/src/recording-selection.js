/** @param {string} kind @param {string} id */
export function recordingSelectionKey(kind,id){return `${kind}:${id}`;}
/** @param {string[]} keys @param {string} kind @param {string} id @param {boolean} checked */
export function toggleRecordingSelection(keys,kind,id,checked){const next=new Set(keys),key=recordingSelectionKey(kind,id);if(checked)next.add(key);else next.delete(key);return [...next];}
/** @param {string[]} keys @param {{kind:string,id:string}[]} entries */
export function pruneRecordingSelection(keys,entries){const valid=new Set(entries.map(item=>recordingSelectionKey(item.kind,item.id)));return [...new Set(keys)].filter(key=>valid.has(key));}
/** Snapshot revisions when confirmation opens; never silently refresh a stale confirmed revision.
 * @param {string[]} keys @param {import('./recording-selection-types').RecordingDeleteItem[]} entries
 * @returns {import('./recording-selection-types').RecordingDeleteItem[]} */
export function selectedRecordingItems(keys,entries){const wanted=new Set(keys);return entries.filter(item=>wanted.has(recordingSelectionKey(item.kind,item.id))).map(item=>({kind:item.kind,id:item.id,...(item.kind==='transcript'?{revision:item.revision}:{})}));}
