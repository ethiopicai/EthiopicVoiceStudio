export type TimedWord = { id: number; word: string; corrected: string; start: number; end: number };
export type Transcript = { created_at_ms?:number; source_recording_id?: string|null; id: string; name: string; model_version: string; text: string; revision: number; words: TimedWord[]; corrections: unknown[] };
