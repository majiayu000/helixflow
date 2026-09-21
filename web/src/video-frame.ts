export type VideoFrameKind = 'current' | 'first' | 'last';

export function frameTimeForKind(
  kind: VideoFrameKind,
  duration: number,
  currentTime: number,
): number {
  const safeDuration = Number.isFinite(duration) && duration > 0 ? duration : 0;
  if (kind === 'first') return Math.min(0.04, Math.max(0, safeDuration * 0.01));
  if (kind === 'last') return Math.max(0, safeDuration - 0.04);
  if (!Number.isFinite(currentTime) || currentTime < 0) return 0;
  return safeDuration > 0 ? Math.min(currentTime, safeDuration) : currentTime;
}

export function liveCanvasVideo(nodeId: string): HTMLVideoElement | null {
  if (typeof document === 'undefined') return null;
  return document.querySelector(`.react-flow__node[data-id="${cssEscape(nodeId)}"] video`);
}

export async function captureVideoFrameBlob(input: {
  url: string;
  kind: VideoFrameKind;
  currentTime?: number;
  live?: HTMLVideoElement | null;
}): Promise<{ blob: Blob; width: number; height: number }> {
  if (input.kind === 'current' && input.live && input.live.videoWidth > 0) {
    return drawVideoFrame(input.live);
  }
  const video = document.createElement('video');
  video.muted = true;
  video.playsInline = true;
  video.preload = 'auto';
  video.crossOrigin = 'anonymous';
  video.src = input.url;
  await waitForEvent(video, 'loadedmetadata');
  video.currentTime = frameTimeForKind(
    input.kind,
    video.duration,
    input.currentTime ?? input.live?.currentTime ?? 0,
  );
  await waitForEvent(video, 'seeked');
  try {
    return await drawVideoFrame(video);
  } finally {
    video.removeAttribute('src');
    video.load();
  }
}

function drawVideoFrame(video: HTMLVideoElement): Promise<{ blob: Blob; width: number; height: number }> {
  const width = video.videoWidth;
  const height = video.videoHeight;
  if (!width || !height) return Promise.reject(new Error('视频还没有可抽的画面'));
  const canvas = document.createElement('canvas');
  canvas.width = width;
  canvas.height = height;
  const context = canvas.getContext('2d');
  if (!context) return Promise.reject(new Error('无法创建抽帧画布'));
  context.drawImage(video, 0, 0, width, height);
  return new Promise((resolve, reject) => {
    canvas.toBlob((blob) => {
      if (!blob) {
        reject(new Error('抽帧失败'));
        return;
      }
      resolve({ blob, width, height });
    }, 'image/jpeg', 0.92);
  });
}

function waitForEvent(video: HTMLVideoElement, type: 'loadedmetadata' | 'seeked'): Promise<void> {
  return new Promise((resolve, reject) => {
    const onError = () => {
      cleanup();
      reject(new Error('视频无法用于抽帧'));
    };
    const onReady = () => {
      cleanup();
      resolve();
    };
    const cleanup = () => {
      video.removeEventListener(type, onReady);
      video.removeEventListener('error', onError);
    };
    video.addEventListener(type, onReady, { once: true });
    video.addEventListener('error', onError, { once: true });
  });
}

function cssEscape(value: string): string {
  return typeof CSS !== 'undefined' && typeof CSS.escape === 'function'
    ? CSS.escape(value)
    : value.replace(/"/g, '\\"');
}
