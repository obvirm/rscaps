import { ALL_FORMATS, BlobSource, Input } from "mediabunny";

export interface VideoProbe {
  durationSec: number;
  fps: number;
  width: number;
  height: number;
}

/**
 * Read a video's duration, frame rate, and display dimensions from its
 * container headers via mediabunny — no frame decoding. Frame rate is
 * deduced from actual packet timestamps (bestGuess snaps to common rates).
 */
export async function probeVideo(file: Blob): Promise<VideoProbe> {
  const input = new Input({ source: new BlobSource(file), formats: ALL_FORMATS });
  try {
    let durationSec = 0;
    try {
      durationSec = (await input.getDurationFromMetadata()) ?? (await input.computeDuration()) ?? 0;
    } catch {
      durationSec = 0;
    }

    let width = 1920;
    let height = 1080;
    let fps = 30;
    try {
      const track = await input.getPrimaryVideoTrack();
      if (track) {
        width = (await track.getDisplayWidth()) || width;
        height = (await track.getDisplayHeight()) || height;
        const metrics = await track.computeFrameRateMetrics();
        fps = Math.round(metrics.bestGuessFrameRate) || 30;
      }
    } catch {
      // keep defaults
    }

    if (!Number.isFinite(durationSec) || durationSec <= 0) durationSec = 0;
    fps = Math.min(Math.max(fps, 1), 120);
    return { durationSec, fps, width, height };
  } finally {
    input.dispose();
  }
}
