import { useRef, type CSSProperties } from 'react';
import type { WorkbenchState } from '../../types';
import { graphNodeWidth, type ViewState } from '../graph-canvas-navigation';

export function MediaCardToolbar({
  canDownload,
  node,
  view,
  onDelete,
  onDownload,
  onDuplicate,
  onReplace,
  onSaveAsset,
}: {
  canDownload: boolean;
  node: WorkbenchState['graph']['nodes'][number];
  view: ViewState;
  onDelete: () => void;
  onDownload: () => void;
  onDuplicate: () => void;
  onReplace: (file: File) => void;
  onSaveAsset: () => void;
}) {
  const replaceRef = useRef<HTMLInputElement | null>(null);
  const left = node.position.x * view.z + view.x + (graphNodeWidth(node) * view.z) / 2;
  const top = node.position.y * view.z + view.y - 10;
  return (
    <div
      className="canvas-image-toolbar"
      style={{ left, top } as CSSProperties}
      onPointerDown={(event) => event.stopPropagation()}
    >
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

