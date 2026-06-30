import { useState, type PointerEvent } from 'react';
import {
  minimapViewportRect,
  type MinimapLayout,
  type ViewState,
  type ViewportSize,
} from './graph-canvas-navigation';

type CanvasMinimapProps = {
  layout: MinimapLayout;
  view: ViewState;
  viewportSize: ViewportSize;
  onNavigate: (x: number, y: number) => void;
};

export function CanvasMinimap({ layout, view, viewportSize, onNavigate }: CanvasMinimapProps) {
  const [dragging, setDragging] = useState(false);
  const viewportRect = minimapViewportRect(layout, view, viewportSize);

  const navigate = (event: PointerEvent<HTMLDivElement>) => {
    const rect = event.currentTarget.getBoundingClientRect();
    onNavigate(event.clientX - rect.left, event.clientY - rect.top);
  };

  return (
    <div
      aria-label="Graph minimap"
      className="canvas-minimap"
      onPointerDown={(event) => {
        event.preventDefault();
        event.stopPropagation();
        event.currentTarget.setPointerCapture(event.pointerId);
        setDragging(true);
        navigate(event);
      }}
      onPointerMove={(event) => {
        if (dragging) navigate(event);
      }}
      onPointerUp={() => setDragging(false)}
      onPointerCancel={() => setDragging(false)}
    >
      {layout.nodes.map((node) => (
        <span
          className="canvas-minimap-node"
          key={node.id}
          style={{
            left: node.x,
            top: node.y,
            width: node.width,
            height: node.height,
          }}
        />
      ))}
      <span
        className="canvas-minimap-viewport"
        style={{
          left: viewportRect.x,
          top: viewportRect.y,
          width: viewportRect.width,
          height: viewportRect.height,
        }}
      />
    </div>
  );
}
