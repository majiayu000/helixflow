import type { CSSProperties } from 'react';
import { Icon } from '../icons';
import type { GraphNodeState } from '../types';
import {
  graphNodeHeight,
  graphNodeWidth,
  type ViewState,
  type ViewportSize,
} from './graph-canvas-navigation';

export function EmptyCanvas() {
  return (
    <div className="empty-canvas">
      <div className="empty-card">
        <div className="empty-icon">
          <Icon n="layers" s={22} />
        </div>
        <div className="empty-title">空白工作流</div>
        <div className="empty-sub">先描述要设计的结果；需要自动化时再生成 workflow。</div>
      </div>
    </div>
  );
}

export function ZoomControls({
  setView,
  view,
}: {
  setView: (updater: (current: ViewState) => ViewState) => void;
  view: ViewState;
}) {
  const zoomPercent = Math.round(view.z * 100);
  const setZoom = (z: number) => {
    setView((current) => ({ ...current, z: clampZoom(z) }));
  };

  return (
    <div className="zoom-ctl" onPointerDown={(event) => event.stopPropagation()}>
      <button aria-label="缩小画布" onClick={() => setZoom(view.z - 0.1)}>
        -
      </button>
      <input
        aria-label="缩放画布"
        max="200"
        min="25"
        onChange={(event) => setZoom(Number(event.currentTarget.value) / 100)}
        type="range"
        value={zoomPercent}
      />
      <button aria-label="放大画布" onClick={() => setZoom(view.z + 0.1)}>
        +
      </button>
      <span>{zoomPercent}%</span>
      <button aria-label="重置画布缩放" onClick={() => setZoom(1)}>
        ⊕
      </button>
    </div>
  );
}

type CanvasGuidesProps = {
  nodes: GraphNodeState[];
  selectedNodes: GraphNodeState[];
  view: ViewState;
  viewportSize: ViewportSize;
};

export function CanvasGuides({
  nodes,
  selectedNodes,
  view,
  viewportSize,
}: CanvasGuidesProps) {
  if (nodes.length === 0 || viewportSize.width < 760) return null;

  const measuredNodes = nodes.map((node) => ({ rect: nodeScreenRect(node, view) }));
  const visibleNodes = measuredNodes.filter(({ rect }) => rectVisible(rect, viewportSize));
  const guideNodes = visibleNodes.length > 0 ? visibleNodes : measuredNodes;
  const leftNode = guideNodes.reduce((best, item) =>
    item.rect.left < best.rect.left ? item : best,
  );
  const rightNode = guideNodes.reduce((best, item) =>
    item.rect.left + item.rect.width > best.rect.left + best.rect.width ? item : best,
  );
  const multiRect = selectedNodes.length > 1 ? nodesScreenBounds(selectedNodes, view, 14) : null;
  const multiLabel = `${selectedNodes.length} selected`;
  const dropStyle = dropZoneStyle(viewportSize);

  return (
    <div className="canvas-guide-overlay" aria-hidden="true">
      {selectedNodes.length === 0 && (
        <GuidePill
          index="1"
          label="工具条：新建文本/图片/视频节点、导入素材"
          style={{
            bottom: 98,
            left: viewportSize.width / 2,
            transform: 'translateX(-50%)',
          }}
        />
      )}
      {multiRect && (
        <span className="guide-selection-frame" style={clampRect(multiRect, viewportSize)}>
          <span className="guide-selection-count">{multiLabel}</span>
        </span>
      )}
      {multiRect && (
        <GuidePill
          index="2"
          label="框选多节点 -> 浮动批量操作"
          style={{
            left: clamp(multiRect.left + multiRect.width / 2 - 94, 120, viewportSize.width - 280),
            top: clamp(multiRect.top + multiRect.height + 16, 142, viewportSize.height - 168),
          }}
        />
      )}
      <GuidePill
        index="3"
        label="节点对话框：围绕选中+上游改写，结果回填"
        style={{
          left: clamp(rightNode.rect.left + rightNode.rect.width + 34, 320, viewportSize.width - 360),
          top: clamp(rightNode.rect.top + rightNode.rect.height + 34, 188, viewportSize.height - 180),
        }}
      />
      <GuidePill
        index="4"
        label="从端口拖出连线，空白处释放即建新节点"
        style={{
          left: clamp(leftNode.rect.left + 440, 360, viewportSize.width - 470),
          top: clamp(viewportSize.height - 190, 360, viewportSize.height - 116),
        }}
      />
      <span className="guide-drop-zone" style={dropStyle}>
        <strong>n6 · 释放创建节点</strong>
        <small>预览 / 保存 / 放大</small>
      </span>
    </div>
  );
}

function GuidePill({
  index,
  label,
  style,
}: {
  index: string;
  label: string;
  style: CSSProperties;
}) {
  return (
    <span className="guide-pill" style={style}>
      <b>{index}</b>
      <span>{label}</span>
    </span>
  );
}

function nodeScreenRect(node: GraphNodeState, view: ViewState) {
  return {
    left: node.position.x * view.z + view.x,
    top: node.position.y * view.z + view.y,
    width: graphNodeWidth(node) * view.z,
    height: graphNodeHeight(node) * view.z,
  };
}

function nodesScreenBounds(nodes: GraphNodeState[], view: ViewState, pad: number) {
  const rects = nodes.map((node) => nodeScreenRect(node, view));
  const left = Math.min(...rects.map((rect) => rect.left));
  const top = Math.min(...rects.map((rect) => rect.top));
  const right = Math.max(...rects.map((rect) => rect.left + rect.width));
  const bottom = Math.max(...rects.map((rect) => rect.top + rect.height));
  return {
    left: left - pad,
    top: top - pad,
    width: right - left + pad * 2,
    height: bottom - top + pad * 2,
  };
}

function clampRect(
  rect: ReturnType<typeof nodesScreenBounds>,
  viewportSize: ViewportSize,
): CSSProperties {
  return {
    left: clamp(rect.left, 14, viewportSize.width - 80),
    top: clamp(rect.top, 118, viewportSize.height - 80),
    width: Math.max(68, Math.min(rect.width, viewportSize.width - 28)),
    height: Math.max(52, Math.min(rect.height, viewportSize.height - 132)),
  };
}

function dropZoneStyle(viewportSize: ViewportSize): CSSProperties {
  return {
    left: clamp(viewportSize.width - 580, 360, viewportSize.width - 246),
    top: clamp(viewportSize.height - 132, 336, viewportSize.height - 82),
  };
}

function rectVisible(
  rect: ReturnType<typeof nodeScreenRect>,
  viewportSize: ViewportSize,
) {
  return (
    rect.left + rect.width > 0 &&
    rect.left < viewportSize.width &&
    rect.top + rect.height > 0 &&
    rect.top < viewportSize.height
  );
}

function clamp(value: number, min: number, max: number) {
  const upper = Math.max(min, max);
  return Math.min(Math.max(value, min), upper);
}

function clampZoom(value: number): number {
  return Math.min(2, Math.max(0.25, value));
}
