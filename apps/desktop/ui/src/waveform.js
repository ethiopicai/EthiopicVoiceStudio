/** Actual signed extrema from PCM channels, bounded to at most maxPeaks columns.
 * @param {ArrayLike<number>[]} channels @param {number} [maxPeaks] */
export function waveformPeaks(channels, maxPeaks = 240) {
  const length = channels.reduce((n, channel) => Math.max(n, channel.length), 0);
  if (!length || !Number.isFinite(maxPeaks) || maxPeaks < 1) return [];
  const count = Math.min(length, Math.floor(maxPeaks));
  return Array.from({ length: count }, (_, index) => {
    const from = Math.floor(index * length / count), to = Math.floor((index + 1) * length / count);
    let min = 0, max = 0;
    for (const channel of channels) for (let i = from; i < Math.min(to, channel.length); i++) {
      const sample = Number.isFinite(channel[i]) ? Math.max(-1, Math.min(1, channel[i])) : 0;
      min = Math.min(min, sample); max = Math.max(max, sample);
    }
    return { min, max };
  });
}
/** Scale display extrema by one shared peak amplitude, keeping zero/silence and
 * relative heights intact. Neither the input peaks nor decoded PCM are changed.
 * @param {{min:number,max:number}[]} peaks */
export function normalizeWaveformPeaks(peaks) {
  /** @param {number} value */
  const finite = value => Number.isFinite(value) ? value : 0;
  const amplitude = peaks.reduce((peak, value) => Math.max(peak, Math.abs(finite(value.min)), Math.abs(finite(value.max))), 0);
  return peaks.map(value => ({
    min: amplitude ? finite(value.min) / amplitude * .99 : 0,
    max: amplitude ? finite(value.max) / amplitude * .99 : 0,
  }));
}
/** @param {number} seconds */
export function formatAudioTime(seconds) {
  const total = Number.isFinite(seconds) ? Math.max(0, Math.floor(seconds)) : 0;
  const hours = Math.floor(total / 3600), minutes = Math.floor(total / 60) % 60;
  return `${hours ? `${hours}:${String(minutes).padStart(2, '0')}` : Math.floor(total / 60)}:${String(total % 60).padStart(2, '0')}`;
}
/** @param {number} value @param {number} duration */
export function boundedSeek(value, duration) {
  if (!Number.isFinite(duration) || duration <= 0 || !Number.isFinite(value)) return 0;
  return Math.max(0, Math.min(duration, value));
}
