import { tauriAPI } from '@/tauri-bridge';
/**
 * Spectrum data for the visualizers.
 *
 * Audio no longer plays through Web Audio, so there is no `AnalyserNode` to
 * read. The Rust player computes the FFT and pushes bars over IPC; this module
 * keeps the latest frame and reference-counts subscribers so the backend only
 * does the work while something is actually drawing.
 */

let bars: Uint8Array = new Uint8Array(64);
let subscribers = 0;
let unlisten: (() => void) | null = null;

export function getSpectrum(): Uint8Array {
  return bars;
}

/** Start receiving spectrum frames; call the returned function to stop. */
export function subscribeSpectrum(): () => void {
  subscribers += 1;
  if (subscribers === 1) {
    const native = tauriAPI.player.native;
    void native?.setSpectrumEnabled(true).catch(() => {});
    unlisten =
      native?.onSpectrum((next) => {
        bars = Uint8Array.from(next);
      }) ?? null;
  }

  let released = false;
  return () => {
    if (released) return;
    released = true;
    subscribers -= 1;
    if (subscribers === 0) {
      unlisten?.();
      unlisten = null;
      bars = new Uint8Array(64);
      void tauriAPI.player.native.setSpectrumEnabled(false).catch(() => {});
    }
  };
}
