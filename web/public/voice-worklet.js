/**
 * AudioWorklet tap for RockServer voice search (served from /voice-worklet.js).
 * It only batches mono input frames and forwards them to the main thread; all
 * rate conversion and Int16 packing happen in src/voicePcm.ts so the DSP stays
 * unit-testable under Node.
 */
class RockserverVoiceTapProcessor extends AudioWorkletProcessor {
  constructor() {
    super();
    this.block = new Float32Array(4096);
    this.filled = 0;
  }

  process(inputs) {
    const channel = inputs[0] && inputs[0][0];
    if (!channel) return true;
    for (let i = 0; i < channel.length; i += 1) {
      this.block[this.filled] = channel[i];
      this.filled += 1;
      if (this.filled === this.block.length) {
        this.port.postMessage(this.block.slice(0));
        this.filled = 0;
      }
    }
    return true;
  }
}

registerProcessor("rockserver-voice-tap", RockserverVoiceTapProcessor);
