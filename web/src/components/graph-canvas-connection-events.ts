import type { PointerEvent } from 'react';
import type { ConnectionPort, PortDirection } from './graph-canvas-connections';

export type PortDropTarget = ConnectionPort & {
  direction: PortDirection;
  index: number;
};

export function releaseConnectionCapture(event: PointerEvent<HTMLElement>) {
  const target = event.target instanceof HTMLElement ? event.target : null;
  if (target?.hasPointerCapture(event.pointerId)) {
    target.releasePointerCapture(event.pointerId);
  }
}

export function portDropTargetFromPoint(clientX: number, clientY: number): PortDropTarget | null {
  if (typeof document === 'undefined') return null;
  const element = document.elementFromPoint(clientX, clientY);
  const port = element?.closest<HTMLElement>('[data-port-node-id]');
  if (!port) return null;
  const direction = port.dataset.portDirection;
  if (direction !== 'input' && direction !== 'output') return null;
  const nodeId = port.dataset.portNodeId;
  const name = port.dataset.portName;
  const type = port.dataset.portType;
  if (!nodeId || !name || !type) return null;
  return {
    nodeId,
    port: name,
    type,
    direction,
    index: Number.parseInt(port.dataset.portIndex ?? '0', 10) || 0,
  };
}

export function confirmReplace(target: ConnectionPort): boolean {
  if (typeof window === 'undefined' || typeof window.confirm !== 'function') return false;
  return window.confirm(`输入端口 ${target.nodeId}.${target.port} 已有连线，是否替换？`);
}
