import { useCallback, useRef, type DragEvent } from 'react';
import type { Connection, IsValidConnection } from '@xyflow/react';
import type {
  CanvasSnapshotUpdate,
  ManualProposalInput,
  NodeDefinition,
  WorkbenchState,
} from '../../types';
import { isVisualMediaCardType } from '../../grid-split';
import {
  buildConnectionProposalInput,
  edgeToRemoveOp,
  findInputConnection,
  matchingConnectionPorts,
  MEDIA_REF_PORT,
  portTypeMatches,
} from '../graph-canvas-connections';
import type { WorkflowFlowEdge, WorkflowFlowInstance, WorkflowFlowNode } from './types';

type FlowActionsInput = {
  versionId: string;
  nodes: WorkbenchState['graph']['nodes'];
  edges: WorkbenchState['graph']['edges'];
  definitionByType: Map<string, NodeDefinition>;
  onCreateProposal?: (input: ManualProposalInput) => Promise<void>;
  onSaveCanvasSnapshot?: (update: CanvasSnapshotUpdate) => Promise<void>;
  onMutationRejected: () => void;
  setStatus: (message: string | null) => void;
};

export function useFlowActions(input: FlowActionsInput) {
  const onCreateProposalRef = useRef(input.onCreateProposal);
  onCreateProposalRef.current = input.onCreateProposal;
  const onSaveCanvasSnapshotRef = useRef(input.onSaveCanvasSnapshot);
  onSaveCanvasSnapshotRef.current = input.onSaveCanvasSnapshot;
  const submit = useCallback(
    (proposal: ManualProposalInput | null, success: string) => {
      if (!proposal || !onCreateProposalRef.current) return;
      void onCreateProposalRef.current(proposal)
        .then(() => input.setStatus(success))
        .catch((error) => {
          input.onMutationRejected();
          input.setStatus(error instanceof Error ? error.message : '画布编辑失败');
        });
    },
    [input.onMutationRejected, input.setStatus],
  );
  const submitSnapshot = useCallback(
    (update: CanvasSnapshotUpdate, success: string) => {
      if (!onSaveCanvasSnapshotRef.current) return;
      void onSaveCanvasSnapshotRef.current(update)
        .then(() => input.setStatus(success))
        .catch((error) => {
          input.onMutationRejected();
          input.setStatus(error instanceof Error ? error.message : '画布布局保存失败');
        });
    },
    [input.onMutationRejected, input.setStatus],
  );

  const commitMove = useCallback(
    (flowNodes: WorkflowFlowNode[]) => {
      const baseById = new Map(input.nodes.map((node) => [node.id, node] as const));
      const positions = flowNodes
        .filter((node) => {
          const base = baseById.get(node.id);
          return base && (base.position.x !== node.position.x || base.position.y !== node.position.y);
        })
        .map((node) => ({ id: node.id, x: node.position.x, y: node.position.y }));
      if (positions.length > 0) {
        submitSnapshot({ positions }, `已移动 ${positions.length} 个节点`);
      }
    },
    [input.nodes, submitSnapshot],
  );

  const commitResize = useCallback(
    (nodeId: string, width: number, height: number) => {
      const base = input.nodes.find((node) => node.id === nodeId);
      if (base?.size?.width === width && base.size.height === height) return;
      submitSnapshot({ sizes: [{ id: nodeId, width, height }] }, `已调整 ${nodeId} 尺寸`);
    },
    [input.nodes, submitSnapshot],
  );

  const connectionPorts = useCallback(
    (connection: {
      source: string;
      target: string;
      sourceHandle?: string | null;
      targetHandle?: string | null;
    }) => {
      if (!connection.source || !connection.target) return null;
      const sourceNode = input.nodes.find((node) => node.id === connection.source);
      const targetNode = input.nodes.find((node) => node.id === connection.target);
      const sourceDefinition = sourceNode ? input.definitionByType.get(sourceNode.nodeType) : undefined;
      const targetDefinition = targetNode ? input.definitionByType.get(targetNode.nodeType) : undefined;
      const sourcePort = connection.sourceHandle
        ? sourceDefinition?.outputs.find((port) => port.name === connection.sourceHandle)
        : undefined;
      const targetPort = connection.targetHandle
        ? targetDefinition?.inputs.find((port) => port.name === connection.targetHandle)
        : undefined;
      if (sourcePort && targetPort) {
        return {
          source: { nodeId: connection.source, port: sourcePort.name, type: sourcePort.type },
          target: { nodeId: connection.target, port: targetPort.name, type: targetPort.type },
        };
      }
      const match = matchingConnectionPorts(sourceDefinition, targetDefinition);
      if (!match) return null;
      return {
        source: { nodeId: connection.source, port: match.sourcePort, type: match.type },
        target: { nodeId: connection.target, port: match.targetPort, type: match.type },
      };
    },
    [input.definitionByType, input.nodes],
  );

  const isValidConnection: IsValidConnection<WorkflowFlowEdge> = useCallback(
    (connection) => {
      const ports = connectionPorts(connection);
      return Boolean(
        ports && (
          portTypeMatches(ports.source.type, ports.target.type)
          || ports.target.port === MEDIA_REF_PORT
        ),
      );
    },
    [connectionPorts],
  );

  const connect = useCallback(
    (connection: Connection) => {
      const ports = connectionPorts(connection);
      if (!ports) {
        input.setStatus('连线失败：端口定义缺失');
        return;
      }
      const targetNode = input.nodes.find((node) => node.id === ports.target.nodeId);
      const targetPort = targetNode
        ? input.definitionByType.get(targetNode.nodeType)?.inputs.find((port) => port.name === ports.target.port)
        : undefined;
      const fanIn = targetPort?.cardinality === 'many';
      const duplicate = input.edges.some((edge) => (
        edge.from.nodeId === ports.source.nodeId
        && edge.from.port === ports.source.port
        && edge.to.nodeId === ports.target.nodeId
        && edge.to.port === ports.target.port
      ));
      if (duplicate) return;
      const existingEdge = fanIn ? undefined : findInputConnection(input.edges, ports.target);
      submit(
        buildConnectionProposalInput({
          baseVersionId: input.versionId,
          source: ports.source,
          target: ports.target,
          existingEdge,
          fanIn,
        }),
        existingEdge ? '已替换输入连线' : '已添加连线',
      );
    },
    [connectionPorts, input.definitionByType, input.edges, input.nodes, input.setStatus, input.versionId, submit],
  );

  const connectNodes = useCallback(
    (sourceId: string, targetId: string) => {
      const sourceNode = input.nodes.find((node) => node.id === sourceId);
      const targetNode = input.nodes.find((node) => node.id === targetId);
      const match = matchingConnectionPorts(
        sourceNode ? input.definitionByType.get(sourceNode.nodeType) : undefined,
        targetNode ? input.definitionByType.get(targetNode.nodeType) : undefined,
      );
      if (!match) {
        input.setStatus('连线失败：这两张卡没有可接的端口');
        return;
      }
      connect({
        source: sourceId,
        target: targetId,
        sourceHandle: match.sourcePort,
        targetHandle: match.targetPort,
      });
    },
    [connect, input.definitionByType, input.nodes, input.setStatus],
  );

  const disconnect = useCallback(
    (edge: WorkflowFlowEdge) => {
      const graphEdge = edge.data?.graphEdge;
      if (!graphEdge) {
        input.setStatus('断开失败：边数据缺失');
        return;
      }
      submit(
        {
          baseVersionId: input.versionId,
          label: `断开 ${graphEdge.from.nodeId} -> ${graphEdge.to.nodeId}`,
          ops: [edgeToRemoveOp(graphEdge)],
        },
        '已断开连线',
      );
    },
    [input.setStatus, input.versionId, submit],
  );

  return { commitMove, commitResize, connect, connectNodes, disconnect, isValidConnection };
}

export function emptyMediaCardId(
  nodeId: string | null | undefined,
  nodes: WorkbenchState['graph']['nodes'],
  isEmpty: (node: WorkbenchState['graph']['nodes'][number]) => boolean,
): string | null {
  if (!nodeId) return null;
  const node = nodes.find((item) => item.id === nodeId);
  if (!node || !isVisualMediaCardType(node.nodeType) || !isEmpty(node)) return null;
  return node.id;
}

export function emptyMediaCardIdFromDropTarget(
  target: EventTarget | null,
  nodes: WorkbenchState['graph']['nodes'],
  isEmpty: (node: WorkbenchState['graph']['nodes'][number]) => boolean,
): string | null {
  if (!(target instanceof Element)) return null;
  return emptyMediaCardId(
    target.closest('.react-flow__node')?.getAttribute('data-id'),
    nodes,
    isEmpty,
  );
}

export function handleFlowDragOver(event: DragEvent, enabled: boolean): void {
  if (!enabled || !event.dataTransfer.types.includes('application/x-helixflow-node-type')) return;
  event.preventDefault();
  event.dataTransfer.dropEffect = 'copy';
}

export function handleFlowDrop(
  event: DragEvent,
  instance: WorkflowFlowInstance | null,
  definitions: Map<string, NodeDefinition>,
  addNode: (definition: NodeDefinition, position: { x: number; y: number }) => void,
): void {
  const nodeType = event.dataTransfer.getData('application/x-helixflow-node-type');
  const definition = definitions.get(nodeType);
  if (!definition || !instance) return;
  event.preventDefault();
  addNode(definition, instance.screenToFlowPosition({ x: event.clientX, y: event.clientY }));
}
