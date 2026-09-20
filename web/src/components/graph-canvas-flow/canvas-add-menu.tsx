import { useRef, type CSSProperties } from 'react';
import type { NodeDefinition } from '../../types';
import type { ViewState } from '../graph-canvas-navigation';

export type CanvasAddMenu = {
  clientX: number;
  clientY: number;
  flow: { x: number; y: number };
  connectFrom?: { nodeId: string; handleType: 'source' | 'target' };
};

const ADD_ITEMS = [
  ['input.text', '文本', '脚本、广告词、品牌文案'],
  ['input.image', '图片', '参考图、生成图'],
  ['input.video', '视频', '首帧、成片'],
  ['input.audio', '音频', '旁白、音效'],
] as const;

export function CanvasAddMenuPanel({
  definitions,
  menu,
  view,
  onAdd,
  onClose,
  onPaste,
  onUpload,
}: {
  definitions: Map<string, NodeDefinition>;
  menu: CanvasAddMenu;
  view: ViewState;
  onAdd: (definition: NodeDefinition) => void;
  onClose: () => void;
  onPaste?: () => void;
  onUpload?: (files: File[]) => void;
}) {
  const uploadRef = useRef<HTMLInputElement | null>(null);
  return (
    <div
      className="canvas-add-menu"
      style={{
        left: menu.flow.x * view.z + view.x,
        top: menu.flow.y * view.z + view.y,
      } as CSSProperties}
      onPointerDown={(event) => event.stopPropagation()}
    >
      <div className="canvas-add-menu-head">添加节点</div>
      {ADD_ITEMS.map(([type, label, hint]) => {
        const definition = definitions.get(type);
        if (!definition) return null;
        return (
          <button key={type} onClick={() => onAdd(definition)} type="button">
            {label}
            <small>{hint}</small>
          </button>
        );
      })}
      {onUpload ? (
        <>
          <div className="canvas-add-menu-sep" />
          <button
            onClick={() => uploadRef.current?.click()}
            type="button"
          >
            上传
            <small>图片 / 视频 / 音频</small>
          </button>
        </>
      ) : null}
      {onPaste ? (
        <button onClick={onPaste} type="button">
          粘贴
          <span className="canvas-add-menu-k">⌘V</span>
        </button>
      ) : null}
      <button className="canvas-add-menu-cancel" onClick={onClose} type="button">
        取消
      </button>
      {onUpload ? (
        <input
          accept="image/*,video/*,audio/*"
          hidden
          multiple
          onChange={(event) => {
            const files = [...(event.currentTarget.files ?? [])];
            event.currentTarget.value = '';
            if (files.length > 0) onUpload(files);
          }}
          ref={uploadRef}
          type="file"
        />
      ) : null}
    </div>
  );
}
