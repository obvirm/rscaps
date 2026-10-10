export interface DecodedAudio {
  sampleRate: 16000;
  samples: Float32Array;
  durationSec: number;
}

/**
 * Decode an audio or video blob to mono 16 kHz PCM (the sample rate whisper
 * expects). Uses Web Audio `decodeAudioData` then resamples/downmixes through
 * an OfflineAudioContext. Browsers extract the audio track from container
 * formats like mp4/aac automatically.
 */
export async function decodeAudioTo16kMono(blob: Blob): Promise<DecodedAudio> {
  const arrayBuf = await blob.arrayBuffer();
  const probeCtx = new (window.AudioContext ||
    (window as unknown as { webkitAudioContext: typeof AudioContext }).webkitAudioContext)();
  let buffer: AudioBuffer;
  try {
    // decodeAudioData detaches the buffer, so pass a copy.
    buffer = await probeCtx.decodeAudioData(arrayBuf.slice(0));
  } finally {
    await probeCtx.close().catch(() => {});
  }
  const durationSec = buffer.duration;
  const targetLength = Math.max(1, Math.ceil(durationSec * 16000));
  // Mono destination downmixes multi-channel input automatically.
  const off = new OfflineAudioContext(1, targetLength, 16000);
  const src = off.createBufferSource();
  src.buffer = buffer;
  src.connect(off.destination);
  src.start();
  const rendered = await off.startRendering();
  return {
    sampleRate: 16000,
    samples: rendered.getChannelData(0).slice(),
    durationSec,
  };
}
