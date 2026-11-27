export type HistoryDeleteItem={kind:string;id:string;revision?:number};
export type HistoryEntry=HistoryDeleteItem&{name:string};
export type HistoryDeleteResult={deleted:HistoryDeleteItem[];failed:(HistoryDeleteItem&{message:string})[]};
