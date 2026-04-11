import { crossfadeEngine } from './AudioCrossfadeEngine';

export function getAnalyser(): AnalyserNode | null {
  return crossfadeEngine.tapAnalyser();
}

export function ensureAnalyser(_mediaElement: HTMLMediaElement): AnalyserNode | null {
  return crossfadeEngine.tapAnalyser();
}

export function resumeAudioContext(): void {
  crossfadeEngine.ensureResumed();
}
