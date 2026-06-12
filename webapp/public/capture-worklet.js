// VisualEars capture worklet — batches mono input into 2048-sample chunks and
// posts them to the main thread. Output stays silent (no echo). Plain JS on
// purpose: AudioWorklet modules are fetched as-is, outside the app bundle.
class VECaptureProcessor extends AudioWorkletProcessor {
  constructor() {
    super();
    this.buf = new Float32Array(2048);
    this.n = 0;
  }

  process(inputs) {
    const ch = inputs[0] && inputs[0][0];
    if (ch) {
      let i = 0;
      while (i < ch.length) {
        const take = Math.min(ch.length - i, this.buf.length - this.n);
        this.buf.set(ch.subarray(i, i + take), this.n);
        this.n += take;
        i += take;
        if (this.n === this.buf.length) {
          const copy = this.buf.slice();
          this.port.postMessage(copy, [copy.buffer]);
          this.n = 0;
        }
      }
    }
    return true;
  }
}

registerProcessor('ve-capture', VECaptureProcessor);
