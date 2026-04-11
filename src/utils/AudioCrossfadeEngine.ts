const CURVE_STEPS = 512;

function buildEqualPowerCurves(): { fadeOut: Float32Array; fadeIn: Float32Array } {
  const fadeOut = new Float32Array(CURVE_STEPS);
  const fadeIn = new Float32Array(CURVE_STEPS);
  for (let i = 0; i < CURVE_STEPS; i++) {
    const t = i / (CURVE_STEPS - 1);
    fadeOut[i] = Math.cos(t * Math.PI * 0.5);
    fadeIn[i] = Math.sin(t * Math.PI * 0.5);
  }
  return { fadeOut, fadeIn };
}

class AudioCrossfadeEngine {
  private ctx: AudioContext | null = null;
  private mainSource: MediaElementAudioSourceNode | null = null;
  private fadeSource: MediaElementAudioSourceNode | null = null;
  private gainA: GainNode | null = null;
  private gainB: GainNode | null = null;
  private masterGain: GainNode | null = null;
  private mainElement: HTMLMediaElement | null = null;
  private _userVolume = 1;
  private _muted = false;
  private _initialized = false;
  private equalPower = buildEqualPowerCurves();
  private _analyserNode: AnalyserNode | null = null;

  get initialized(): boolean {
    return this._initialized;
  }

  init(element: HTMLMediaElement): boolean {
    if (this._initialized && this.mainElement === element) {
      this.ensureResumed();
      return true;
    }
    this.dispose();

    try {
      this.ctx = new AudioContext();
      this.ensureResumed();

      this.mainSource = this.ctx.createMediaElementSource(element);
      this.gainA = this.ctx.createGain();
      this.gainB = this.ctx.createGain();
      this.masterGain = this.ctx.createGain();

      this.gainA.gain.value = 1;
      this.gainB.gain.value = 0;
      this.masterGain.gain.value = this._muted ? 0 : this._userVolume;

      this.mainSource.connect(this.gainA);
      this.gainA.connect(this.masterGain);
      this.gainB.connect(this.masterGain);
      this.masterGain.connect(this.ctx.destination);

      element.volume = 1;
      this.mainElement = element;
      this._initialized = true;
      return true;
    } catch {
      this.dispose();
      return false;
    }
  }

  prepareFade(fadeAudio: HTMLAudioElement): boolean {
    if (!this.ctx || !this.gainB) return false;
    this.disposeFade();

    try {
      this.fadeSource = this.ctx.createMediaElementSource(fadeAudio);
      this.fadeSource.connect(this.gainB);
      fadeAudio.volume = 1;
      return true;
    } catch {
      return false;
    }
  }

  startCrossfade(durationSec: number): void {
    if (!this.ctx || !this.gainA || !this.gainB) return;
    this.ensureResumed();

    const now = this.ctx.currentTime;
    this.gainA.gain.cancelScheduledValues(now);
    this.gainB.gain.cancelScheduledValues(now);
    this.gainA.gain.setValueAtTime(1, now);
    this.gainB.gain.setValueAtTime(0, now);

    try {
      this.gainA.gain.setValueCurveAtTime(this.equalPower.fadeOut, now, durationSec);
      this.gainB.gain.setValueCurveAtTime(this.equalPower.fadeIn, now, durationSec);
    } catch {
      this.gainA.gain.linearRampToValueAtTime(0, now + durationSec);
      this.gainB.gain.linearRampToValueAtTime(1, now + durationSec);
    }
  }

  completeCrossfade(): void {
    if (!this.ctx || !this.gainA || !this.gainB) return;

    const now = this.ctx.currentTime;
    this.gainA.gain.cancelScheduledValues(now);
    this.gainB.gain.cancelScheduledValues(now);
    this.gainA.gain.setValueAtTime(1, now);
    this.gainB.gain.setValueAtTime(0, now);
    this.disposeFade();
  }

  abortCrossfade(): void {
    this.completeCrossfade();
  }

  setVolume(v: number): void {
    this._userVolume = v;
    if (this.masterGain && !this._muted) {
      this.masterGain.gain.value = v;
    }
    if (!this._initialized && this.mainElement) {
      this.mainElement.volume = v;
    }
  }

  setMuted(m: boolean): void {
    this._muted = m;
    if (this.masterGain) {
      this.masterGain.gain.value = m ? 0 : this._userVolume;
    }
    if (!this._initialized && this.mainElement) {
      this.mainElement.muted = m;
    }
  }

  ensureResumed(): void {
    if (this.ctx && this.ctx.state === 'suspended') {
      this.ctx.resume().catch(() => {});
    }
  }

  suspend(): void {
    if (this.ctx && this.ctx.state === 'running') {
      this.ctx.suspend().catch(() => {});
    }
  }

  getHardwareLatency(): number {
    if (!this.ctx) return 0;
    const stamp = this.ctx.getOutputTimestamp?.();
    if (!stamp || stamp.contextTime === 0) return 0;
    const lag = this.ctx.currentTime - stamp.contextTime;
    return Math.min(0.5, Math.max(0, lag));
  }

  /**
   * Insert an AnalyserNode between masterGain and destination so visualizers
   * can read frequency data without creating a second MediaElementAudioSourceNode.
   * Idempotent — returns the same node on repeated calls.
   */
  tapAnalyser(fftSize = 512, smoothingTimeConstant = 0.75): AnalyserNode | null {
    if (this._analyserNode) return this._analyserNode;
    if (!this.ctx || !this.masterGain) return null;
    try {
      this._analyserNode = this.ctx.createAnalyser();
      this._analyserNode.fftSize = fftSize;
      this._analyserNode.smoothingTimeConstant = smoothingTimeConstant;
      this.masterGain.connect(this._analyserNode);
      return this._analyserNode;
    } catch {
      this._analyserNode = null;
      return null;
    }
  }

  disposeFade(): void {
    if (this.fadeSource) {
      try { this.fadeSource.disconnect(); } catch {} // eslint-disable-line no-empty
      this.fadeSource = null;
    }
  }

  dispose(): void {
    this.disposeFade();
    if (this._analyserNode) {
      try { this._analyserNode.disconnect(); } catch {} // eslint-disable-line no-empty
      this._analyserNode = null;
    }
    if (this.mainSource) {
      try { this.mainSource.disconnect(); } catch {} // eslint-disable-line no-empty
      this.mainSource = null;
    }
    if (this.gainA) {
      try { this.gainA.disconnect(); } catch {} // eslint-disable-line no-empty
      this.gainA = null;
    }
    if (this.gainB) {
      try { this.gainB.disconnect(); } catch {} // eslint-disable-line no-empty
      this.gainB = null;
    }
    if (this.masterGain) {
      try { this.masterGain.disconnect(); } catch {} // eslint-disable-line no-empty
      this.masterGain = null;
    }
    if (this.ctx && this.ctx.state !== 'closed') {
      this.ctx.close().catch(() => {});
      this.ctx = null;
    }
    this.mainElement = null;
    this._initialized = false;
  }
}

export const crossfadeEngine = new AudioCrossfadeEngine();
