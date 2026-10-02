/** PCM speech-end heuristic matching RockCast's recording thresholds and timing. */
export class SpeechEndDetector {
  private samples = 0;
  private windowSamples = 0;
  private squares = 0;
  private noise = Infinity;
  private firstSpeech = 0;
  private lastSpeech = 0;
  outcome: "speech" | "silence" | null = null;

  /** Analyze fixed 100 ms windows regardless of microphone callback chunk size. */
  push(pcm: Int16Array): void {
    for (const sample of pcm) {
      if (this.outcome) return;
      this.samples += 1;
      this.squares += sample * sample;
      if (++this.windowSamples < 1600) continue;
      const rms = Math.sqrt(this.squares / this.windowSamples);
      this.squares = 0;
      this.windowSamples = 0;
      const elapsed = this.samples / 16; // 16 kHz PCM, milliseconds
      if (elapsed < 350) {
        this.noise = Math.min(this.noise, rms);
        continue;
      }
      const threshold = Number.isFinite(this.noise)
        ? Math.min(520, Math.max(280, this.noise + 140)) : 340;
      // ponytail: amplitude heuristic also detects loud background noise;
      // use a speech classifier if real-device testing shows false triggers.
      if (rms >= threshold) {
        this.firstSpeech ||= elapsed;
        this.lastSpeech = elapsed;
      }
      if (this.firstSpeech) {
        if (elapsed - this.lastSpeech >= 1000 || elapsed - this.firstSpeech >= 8000)
          this.outcome = "speech";
      } else if (elapsed >= 4500) this.outcome = "silence";
    }
  }
}
