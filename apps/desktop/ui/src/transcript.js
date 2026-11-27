import { timedTokenIndexAt } from './timed-text.js';
export const MAX_TRANSCRIPTION_BYTES = 100 * 1024 * 1024;
/** @param {{name:string,size:number}} file */
export function transcriptionFileError(file) {
  if (!Number.isFinite(file.size) || file.size <= 0) return 'This file is empty. Choose another recording.';
  if (file.size > MAX_TRANSCRIPTION_BYTES) return 'Choose a recording no larger than 100 MB.';
  if (!/\.(wav|mp3|flac|ogg|m4a)$/i.test(file.name)) return 'Choose a WAV, MP3, FLAC, OGG, or M4A recording.';
  return '';
}
/** @param {string} text */
export function correctionError(text) {
  if (!text.trim()) return 'Enter a replacement word before saving.';
  if (text.trim().length > 200) return 'Keep this correction to 200 characters or fewer.';
  return '';
}
/** @param {{start:number,end:number}} word */
export function playbackSpan(word) {
  return Number.isFinite(word.start) && Number.isFinite(word.end) && word.start >= 0 && word.end > word.start ? { start: word.start, end: word.end } : null;
}
/** @param {string} state */
export function isActiveJob(state) { return ['queued', 'running', 'loading', 'transcribing', 'cancelling'].includes(state); }

/** Word intervals are chronological and nonoverlapping, as validated by native storage.
 * Start-inclusive/end-exclusive: pauses and the final end never highlight a word.
 * @param {{id:number,start:number,end:number}[]} words @param {number} seconds
 * @returns {number|null} */
export function timedWordIdAt(words, seconds) {
  const index=timedTokenIndexAt(words,seconds);
  return index===null?null:words[index].id;
}
