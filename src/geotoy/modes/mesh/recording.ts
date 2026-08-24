import * as THREE from 'three';
import { get, writable, type Readable } from 'svelte/store';
import type { Viz } from 'src/viz';
import type { GeoscriptPlaygroundUserData } from 'src/viz/scenes/geoscriptPlayground/geoscriptPlayground.svelte';
import { AsyncOnce } from 'src/viz/util/AsyncOnce';
import { logGeotoyEvent } from 'src/analytics';

const canvasRecordModule = import.meta.env.SSR ? null : new AsyncOnce(() => import('canvas-record'));
const mediaCodecsModule = import.meta.env.SSR ? null : new AsyncOnce(() => import('media-codecs'));
const mediabunnyModule = import.meta.env.SSR ? null : new AsyncOnce(() => import('mediabunny'));

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
  let releaseLoopLease: (() => void) | null = null;
  const recordingState = writable<RecordingState>('not-recording');

  const toggleRecording = async () => {
    const currentRecordingState = get(recordingState);

    if (currentRecordingState === 'initializing') {
      return;
    } else if (currentRecordingState === 'recording') {
      recordingState.set('initializing');
      try {
        await recorder?.stop();
      } catch (err) {
        // Surfaced rather than rethrown: every call site drops the promise, so an unhandled
        // rejection would leave the button back at idle looking like a successful save.
        console.error('Failed to finish recording', err);
        alert('Recording failed to save. See the console for details.');
      } finally {
        // In a `finally` because a leaked lease pins the render loop at full rate for the rest
        // of the session with no visible symptom.
        if (afterRenderCb) {
          viz.unregisterAfterRenderCb(afterRenderCb);
          afterRenderCb = null;
        }
        releaseLoopLease?.();
        releaseLoopLease = null;
        recordingState.set('not-recording');
        recorder = null;
      }
      return;
    }

    recordingState.set('initializing');
    try {
      if (!canvasRecordModule || !mediaCodecsModule || !mediabunnyModule) {
        recordingState.set('not-recording');
        throw new Error('Recording is only available in the browser runtime.');
      }

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
      const av1Support = await VideoEncoder.isConfigSupported({
        codec: av1Codec,
        width,
        height,
        bitrateMode: 'variable',
        hardwareAcceleration: 'prefer-hardware',
        latencyMode: 'realtime',
        bitrate,
        framerate: fps,
      });
      if (!av1Support.supported) {
        console.warn('AV1 codec not supported, falling back to AVC');
        codec = avcCodec;
        const avcSupport = await VideoEncoder.isConfigSupported({
          codec: avcCodec,
          width,
          height,
          bitrate,
          framerate: fps,
        });
        if (!avcSupport.supported) {
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

      // Assigned before the await so a `start` failure is reachable by the cleanup below.
      recorder = newRecorder;
      await newRecorder.start({ filename });
      // Reads the canvas, so it needs frames even while the 3D view is covered.
      releaseLoopLease = viz.frameGovernor?.acquireContinuous(true) ?? null;
      recordingState.set('recording');
      logGeotoyEvent('composition', 'record_start', { comp_id: compositionId });

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
    } finally {
      // Keyed on the callback, not on `recorder`: a throw between assigning the recorder and
      // registering the callback would otherwise leave the toggle reading 'recording' with a
      // lease held and nothing ever captured.
      if (!afterRenderCb) {
        // Stopped before dropping the reference, or a started encoder would keep muxing with
        // nothing left able to reach it.
        void recorder?.stop().catch(() => 0);
        releaseLoopLease?.();
        releaseLoopLease = null;
        recorder = null;
        recordingState.set('not-recording');
      }
    }
  };

  return { toggleRecording, recordingState };
};
