/** Seconds use the same origin as the host player's playback clock. */
export type TimedTextToken = {
  id?: string | number;
  text: string;
  start: number;
  end: number;
  /** Optional presentation hint for text edited by the host. */
  changed?: boolean;
};
