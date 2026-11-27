export const MAX_REFERENCE_BYTES = 50 * 1024 * 1024;

/** @param {{name: string, size: number}} file */
export function referenceError(file) {
  if (file.size === 0) return 'This file is empty. Choose another recording.';
  if (file.size > MAX_REFERENCE_BYTES) return 'Choose a recording smaller than 50 MB.';
  if (!/\.(wav|mp3|flac|ogg|m4a)$/i.test(file.name)) {
    return 'Choose a WAV, MP3, FLAC, OGG, or M4A audio file.';
  }
  return '';
}
