import { useRef, useState } from 'react';
import type { WorkbenchState } from '../../types';
import type { ViewState } from '../graph-canvas-navigation';
import { flowAboveCenterStyle } from './overlay-anchor';
import type { VideoFrameKind } from '../../video-frame';

export function MediaCardToolbar({
  canDownload,
  node,
  view: _view,
  onDelete,
  onDownload,
  onDuplicate,
  onExtractFrame,
  onReplace,
  onSaveAsset,
}: {
  canDownload: boolean;
  node: WorkbenchState['graph']['nodes'][number];
  view: ViewState;
  onDelete: () => void;
  onDownload: () => void;
  onDuplicate: () => void;
  onExtractFrame?: (kind: VideoFrameKind) => void;
  onReplace: (file: File) => void;
  onSaveAsset: () => void;
}) {
  const replaceRef = useRef<HTMLInputElement | null>(null);
  const [extractOpen, setExtractOpen] = useState(false);
  const video = Boolean(onExtractFrame);
  return (
    <div
      className="canvas-image-toolbar nodrag nopan"
      style={flowAboveCenterStyle(node, 10)}
      onPointerDown={(event) => event.stopPropagation()}
    >
      {video ? (
        <div className="canvas-image-toolbar-split">
          <button onClick={() => setExtractOpen((open) => !open)} type="button">抽帧</button>
          {extractOpen && (
            <div className="canvas-image-toolbar-menu">
              <button onClick={() => { setExtractOpen(false); onExtractFrame?.('current'); }} type="button">当前帧</button>
              <button onClick={() => { setExtractOpen(false); onExtractFrame?.('first'); }} type="button">首帧</button>
              <button onClick={() => { setExtractOpen(false); onExtractFrame?.('last'); }} type="button">末帧</button>
            </div>
          )}
        </div>
      ) : null}
      <button onClick={() => replaceRef.current?.click()} type="button">替换</button>
      <button onClick={onDuplicate} type="button">复制</button>
      <button disabled={!canDownload} onClick={onDownload} type="button">下载</button>
      <button onClick={onSaveAsset} type="button">入库</button>
      <button onClick={onDelete} type="button">删除</button>
      <input
        accept="image/*,video/*,audio/*"
        hidden
        onChange={(event) => {
          const file = event.currentTarget.files?.[0];
          event.currentTarget.value = '';
          if (file) onReplace(file);
        }}
        ref={replaceRef}
        type="file"
      />
    </div>
  );
}
