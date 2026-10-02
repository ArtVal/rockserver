/**
 * Pure PCM conversion for the voice stream contract: the browser microphone
 * runs at its native rate (commonly 44.1 or 48 kHz), while
 * `/api/v1/voice/stream` accepts only signed 16-bit little-endian mono PCM at
 * exactly 16 000 Hz. This module downsamples arbitrary input rates with a
 * windowed-sinc low-pass so decimation cannot alias speech harmonics, and
 * converts floats to Int16. It has no DOM dependencies and is unit-testable
 * under Node.
 */

/** Target sample rate required by the voice stream contract. */
export const VOICE_SAMPLE_RATE_HZ = 16_000;

/** Window half-width in input samples; 64 taps keep the transition band narrow for speech. */
const HALF_TAPS = 32;

const sinc = (t: number) => (t === 0 ? 1 : Math.sin(Math.PI * t) / (Math.PI * t));

const blackman = (u: number) => 0.42 + 0.5 * Math.cos(Math.PI * u) + 0.08 * Math.cos(2 * Math.PI * u);

const toInt16 = (value: number) =>
  Math.max(-32_768, Math.min(32_767, Math.round(value * 32_768)));

/**
 * Streaming anti-aliased resampler from any input rate to 16 kHz mono Int16.
 *
 * Each output sample is a windowed-sinc interpolation of the input around its
 * fractional position, with the ideal low-pass cutoff scaled to the output
 * Nyquist frequency, so components above 8 kHz are attenuated before
 * decimation. Weights are normalized per sample, giving exact unity DC gain
 * regardless of the fractional phase. The last `2 * HALF_TAPS - 1` input
 * samples are retained between calls; the sub-millisecond tail that would need
 * samples past the block end is intentionally dropped rather than zero-padded.
 */
export class PcmDownsampler {
  private readonly ratio: number;
  private readonly cutoff: number;
  private buffer: number[] = [];
  /** Absolute input index of `buffer[0]`; only grows by trimming. */
  private bufferStart = 0;
  /** Absolute fractional input position of the next output sample. */
  private nextOut = 0;

  constructor(inputRate: number, outputRate: number = VOICE_SAMPLE_RATE_HZ) {
    if (!Number.isFinite(inputRate) || inputRate <= 0)
      throw new Error(`invalid microphone sample rate: ${inputRate}`);
    if (!Number.isFinite(outputRate) || outputRate <= 0)
      throw new Error(`invalid output sample rate: ${outputRate}`);
    this.ratio = inputRate / outputRate;
    // Passband edge in cycles per input sample; never above the input Nyquist.
    this.cutoff = Math.min(0.5 * (outputRate / inputRate), 0.5);
  }

  /** Resamples one Float32 input block in [-1, 1] and returns Int16 output samples. */
  push(block: Float32Array): Int16Array {
    this.buffer.push(...block);
    const outputs: number[] = [];
    // The interpolation window [center - HALF_TAPS + 1, center + HALF_TAPS - 1]
    // must fit inside the buffered input; taps before the stream start are
    // skipped, which zero-extends the signal for the first sub-millisecond.
    for (;;) {
      const center = Math.floor(this.nextOut);
      const first = Math.max(center - HALF_TAPS + 1, this.bufferStart);
      const last = center + HALF_TAPS - 1;
      if (last - this.bufferStart >= this.buffer.length) break;
      const t = this.nextOut;
      let weighted = 0;
      let weightSum = 0;
      for (let k = first; k <= last; k += 1) {
        const offset = k - t; // distance in input samples
        if (offset <= -HALF_TAPS || offset >= HALF_TAPS) continue;
        const weight =
          2 * this.cutoff * sinc(2 * this.cutoff * offset) * blackman(offset / HALF_TAPS);
        if (weight === 0) continue;
        weighted += weight * this.buffer[k - this.bufferStart];
        weightSum += weight;
      }
      outputs.push(weightSum > 0 ? toInt16(weighted / weightSum) : 0);
      this.nextOut += this.ratio;
    }
    // Trim samples the next window can no longer reference.
    const keepFrom = Math.floor(this.nextOut) - HALF_TAPS + 1;
    if (keepFrom > this.bufferStart) {
      this.buffer.splice(0, keepFrom - this.bufferStart);
      this.bufferStart = keepFrom;
    }
    return Int16Array.from(outputs);
  }
}
