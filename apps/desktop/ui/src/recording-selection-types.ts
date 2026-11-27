export type RecordingKind = 'transcript' | 'recording';
export type RecordingDeleteItem = {kind:RecordingKind;id:string;revision?:number};
export type RecordingSelectionEntry = RecordingDeleteItem & {name:string};
export type RecordingBatchDeleteResult = {
  deleted:{kind:RecordingKind;id:string}[];
  failed:{kind:RecordingKind;id:string;message:string}[];
  removedRecordingIds:string[];
};
