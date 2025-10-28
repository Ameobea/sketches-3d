import * as THREE from 'three';
import { get, writable, type Readable } from 'svelte/store';
import type { Viz } from 'src/viz';
import type { GeoscriptPlaygroundUserData } from './geoscriptPlayground.svelte';
import { AsyncOnce } from 'src/viz/util/AsyncOnce';

const canvasRecordModule = new AsyncOnce(() => import('canvas-record'));
const mediaCodecsModule = new AsyncOnce(() => import('media-codecs'));
const mediabunnyModule = new AsyncOnce(() => import('mediabunny'));

export type RecordingState = 'not-recording' | 'initializing' | 'recording';

export const useRecording = (
  viz: Viz,
  userData: GeoscriptPlaygroundUserData | undefined
): {
  toggleRecording: () => Promise<void>;
  recordingState: Readable<RecordingState>;
} => {
  let recorder: any | null = null;
  let afterRenderCb: ((curTimeSeconds: number) => void) | null = null;
  const recordingState = writable<RecordingState>('not-recording');

  const toggleRecording = async () => {
    let currentRecordingState = get(recordingState);

    if (currentRecordingState === 'initializing') {
      return;
    } else if (currentRecordingState === 'recording') {
      recordingState.set('initializing');
      await recorder?.stop();
      if (afterRenderCb) {
        viz.unregisterAfterRenderCb(afterRenderCb);
        afterRenderCb = null;
      }
      recordingState.set('not-recording');
      recorder = null;
      return;
    }

    recordingState.set('initializing');

    const { Recorder, RecorderStatus, isWebCodecsSupported } = await canvasRecordModule.get();
    const { AVC, AV } = await mediaCodecsModule.get();
    const { Mp4OutputFormat } = await mediabunnyModule.get();

    if (!isWebCodecsSupported) {
      alert('WebCodecs is not supported in this browser. Cannot record video.');
      return;
    }

    const { width, height } = viz.renderer.getSize(new THREE.Vector2());

    const compositionId = userData?.initialComposition?.comp.id ?? 'local';

    const now = new Date();
    const dateStr = `${now.getFullYear()}${String(now.getMonth() + 1).padStart(2, '0')}${String(
      now.getDate()
    ).padStart(2, '0')}_${String(now.getHours()).padStart(2, '0')}${String(now.getMinutes()).padStart(
      2,
      '0'
    )}${String(now.getSeconds()).padStart(2, '0')}`;

    const filename = `geotoy_${compositionId}_${dateStr}.mp4`;

    const bitrate = 12_000_000;
    const fps = 60;

    const av1Codec = AV.getCodec({ profile: 'High', level: '5.2', bitDepth: 8, tier: 'High', name: 'AV1' });
    const avcCodec = AVC.getCodec({ profile: 'High', level: '5.2' });
    let codec = av1Codec;
    if (
      !VideoEncoder.isConfigSupported({
        codec: av1Codec,
        width,
        height,
        bitrateMode: 'variable',
        hardwareAcceleration: 'prefer-hardware',
        latencyMode: 'realtime',
        bitrate,
        framerate: fps,
      })
    ) {
      console.warn('AV1 codec not supported, falling back to AVC');
      codec = avcCodec;
      if (
        !VideoEncoder.isConfigSupported({
          codec: avcCodec,
          width,
          height,
          bitrate,
          framerate: fps,
        })
      ) {
        alert('Neither AV1 nor AVC codecs are supported in this browser. Cannot record video.');
        return;
      }
    }

    console.log(`Recording with codec: ${codec}`);

    const newRecorder = new Recorder(viz.renderer.getContext(), {
      name: filename,
      encoderOptions: {
        codec,
        width,
        height,
        bitrate,
      },
      // Working around a bug in `canvas-recorder` when using AV1.
      //
      // It assumes that .mp4 files are ISO BMFF files, but AV1 is not that.
      //
      // So, we have to lie to `canvas-recorder` about the file extension but give the
      // correct one to `mediabunny` in `muxerOptions`.
      extension: codec.startsWith('av01') ? 'mkv' : 'mp4',
      muxerOptions: { format: new Mp4OutputFormat({ fastStart: 'in-memory' }) },
      frameRate: fps,
      duration: Infinity,
      download: true,
    });

    await newRecorder.start({ filename });
    recorder = newRecorder;
    recordingState.set('recording');

    let lastFrameTime = 0;
    afterRenderCb = (curTimeSeconds: number) => {
      if (recorder && recorder.status === RecorderStatus.Recording) {
        if (lastFrameTime === 0) {
          lastFrameTime = curTimeSeconds;
        } else if (curTimeSeconds - lastFrameTime < 1 / fps) {
          // Throttle to 60 FPS
          return;
        }
        lastFrameTime = curTimeSeconds;
        recorder.step();
      }
    };
    viz.registerAfterRenderCb(afterRenderCb);
  };

  return { toggleRecording, recordingState };
};
