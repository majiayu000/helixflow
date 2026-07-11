import { useCallback, useEffect, useMemo, useState, type PointerEvent } from 'react';
import type { GraphNodeState, NodeCatalog, WorkbenchState } from '../types';
import {
  buildConnectionProposalInput,
  buildPortHighlights,
  edgeToRemoveOp,
  findInputConnection,
  portAnchorPoint,
  portTypeMatches,
} from './graph-canvas-connections';
import {
  confirmReplace,
  portDropTargetFromPoint,
  releaseConnectionCapture,
} from './graph-canvas-connection-events';
import type { GraphCanvasProps, ConnectionDragState } from './graph-canvas-types';
import type { Point } from './graph-canvas-selection';

type ConnectionControllerInput = {
  canConnect: boolean;
  definitions: Map<string, NodeCatalog['nodes'][number]>;
  edges: WorkbenchState['graph']['edges'];
  nodes: GraphNodeState[];
  onCreateProposal: GraphCanvasProps['onCreateProposal'];
  versionId: string;
  worldPointFromClient: (clientX: number, clientY: number) => Point;
};

export function useCanvasConnectionController({
  canConnect,
  definitions,
  edges,
  nodes,
  onCreateProposal,
  versionId,
  worldPointFromClient,
}: ConnectionControllerInput) {
  const [drag, setDrag] = useState<ConnectionDragState | null>(null);
  const [status, setStatus] = useState<string | null>(null);
  const portHighlights = useMemo(
    () =>
      drag
        ? buildPortHighlights(nodes, definitions, drag.source, edges)
        : new Map(),
    [definitions, drag, edges, nodes],
  );

  const reset = useCallback(() => {
    setDrag(null);
    setStatus(null);
  }, []);

  useEffect(() => {
    if (!canConnect) setDrag(null);
  }, [canConnect]);

  const start = (
    node: GraphNodeState,
    port: { name: string; type: string },
    index: number,
    event: PointerEvent<HTMLSpanElement>,
  ) => {
    if (!canConnect || event.button !== 0) return;
    event.preventDefault();
    event.stopPropagation();
    event.currentTarget.setPointerCapture(event.pointerId);
    setStatus(null);
    setDrag({
      pointerId: event.pointerId,
      source: { nodeId: node.id, port: port.name, type: port.type },
      sourcePoint: portAnchorPoint(node, 'output', index),
      currentPoint: worldPointFromClient(event.clientX, event.clientY),
    });
  };

  const cancel = (event: PointerEvent<HTMLElement>) => {
    if (!drag || drag.pointerId !== event.pointerId) return false;
    releaseConnectionCapture(event);
    setDrag(null);
    if (canConnect) setStatus('连线已取消');
    return true;
  };

  const complete = (event: PointerEvent<HTMLElement>) => {
    if (!drag || drag.pointerId !== event.pointerId) return false;
    event.preventDefault();
    event.stopPropagation();
    releaseConnectionCapture(event);
    setDrag(null);
    if (!canConnect) return true;

    const target = portDropTargetFromPoint(event.clientX, event.clientY);
    if (!target || target.direction !== 'input') {
      setStatus('未连接：请选择输入端口');
      return true;
    }
    if (!portTypeMatches(drag.source.type, target.type)) {
      setStatus('端口类型不兼容');
      return true;
    }

    const existingEdge = findInputConnection(edges, target);
    const proposal = buildConnectionProposalInput({
      baseVersionId: versionId,
      source: drag.source,
      target,
      existingEdge,
    });
    if (!proposal) {
      setStatus('连线未变化');
      return true;
    }
    if (existingEdge && proposal.ops.length > 1 && !confirmReplace(target)) {
      setStatus('已取消替换');
      return true;
    }

    void onCreateProposal?.(proposal)
      .then(() =>
        setStatus(existingEdge ? '已加入编辑会话：替换连线' : '已加入编辑会话：连接端口'),
      )
      .catch((error) => {
        setStatus(error instanceof Error ? error.message : '连线提交失败');
      });
    return true;
  };

  const disconnect = (edge: WorkbenchState['graph']['edges'][number]) => {
    if (!onCreateProposal || !canConnect) return;
    void onCreateProposal({
      baseVersionId: versionId,
      label: `断开 ${edge.from.nodeId}.${edge.from.port} -> ${edge.to.nodeId}.${edge.to.port}`,
      ops: [edgeToRemoveOp(edge)],
    })
      .then(() => setStatus('已加入编辑会话：断开连线'))
      .catch((error) => {
        setStatus(error instanceof Error ? error.message : '断线提交失败');
      });
  };

  return {
    cancel,
    complete,
    disconnect,
    drag,
    portHighlights,
    reset,
    setDrag,
    setStatus,
    start,
    status,
  };
}
