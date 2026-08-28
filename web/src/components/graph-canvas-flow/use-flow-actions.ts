import { useCallback, useRef, type DragEvent } from 'react';
import type { Connection, IsValidConnection } from '@xyflow/react';
import type { ManualProposalInput, NodeDefinition, WorkbenchState } from '../../types';
import { buildMoveNodeEditInput, buildResizeNodeEditInput } from '../../workbench-edit-session';
import {
  buildConnectionProposalInput,
  edgeToRemoveOp,
  findInputConnection,
  portTypeMatches,
} from '../graph-canvas-connections';
import type { WorkflowFlowEdge, WorkflowFlowInstance, WorkflowFlowNode } from './types';

type FlowActionsInput = {
  versionId: string;
  nodes: WorkbenchState['graph']['nodes'];
  edges: WorkbenchState['graph']['edges'];
  definitionByType: Map<string, NodeDefinition>;
  onCreateProposal?: (input: ManualProposalInput) => Promise<void>;
  setStatus: (message: string | null) => void;
};

export function useFlowActions(input: FlowActionsInput) {
  const onCreateProposalRef = useRef(input.onCreateProposal);
  onCreateProposalRef.current = input.onCreateProposal;
  const submit = useCallback(
    (proposal: ManualProposalInput | null, success: string) => {
      if (!proposal || !onCreateProposalRef.current) return;
      void onCreateProposalRef.current(proposal)
        .then(() => input.setStatus(success))
        .catch((error) => {
          input.setStatus(error instanceof Error ? error.message : '画布编辑失败');
        });
    },
    [input.setStatus],
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
      submit(buildMoveNodeEditInput(input.versionId, positions), `已移动 ${positions.length} 个节点`);
    },
    [input.nodes, input.versionId, submit],
  );

  const commitResize = useCallback(
    (nodeId: string, width: number, height: number) => {
      const base = input.nodes.find((node) => node.id === nodeId);
      if (base?.size?.width === width && base.size.height === height) return;
      submit(
        buildResizeNodeEditInput(input.versionId, [{ id: nodeId, width, height }]),
        `已调整 ${nodeId} 尺寸`,
      );
    },
    [input.nodes, input.versionId, submit],
  );

  const connectionPorts = useCallback(
    (connection: {
      source: string;
      target: string;
      sourceHandle?: string | null;
      targetHandle?: string | null;
    }) => {
      if (!connection.source || !connection.target || !connection.sourceHandle || !connection.targetHandle) {
        return null;
      }
      const sourceNode = input.nodes.find((node) => node.id === connection.source);
      const targetNode = input.nodes.find((node) => node.id === connection.target);
      const sourcePort = sourceNode
        ? input.definitionByType.get(sourceNode.nodeType)?.outputs.find(
            (port) => port.name === connection.sourceHandle,
          )
        : undefined;
      const targetPort = targetNode
        ? input.definitionByType.get(targetNode.nodeType)?.inputs.find(
            (port) => port.name === connection.targetHandle,
          )
        : undefined;
      if (!sourcePort || !targetPort) return null;
      return {
        source: { nodeId: connection.source, port: sourcePort.name, type: sourcePort.type },
        target: { nodeId: connection.target, port: targetPort.name, type: targetPort.type },
      };
    },
    [input.definitionByType, input.nodes],
  );

  const isValidConnection: IsValidConnection<WorkflowFlowEdge> = useCallback(
    (connection) => {
      const ports = connectionPorts(connection);
      return Boolean(ports && portTypeMatches(ports.source.type, ports.target.type));
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
      const existingEdge = findInputConnection(input.edges, ports.target);
      submit(
        buildConnectionProposalInput({
          baseVersionId: input.versionId,
          source: ports.source,
          target: ports.target,
          existingEdge,
        }),
        existingEdge ? '已替换输入连线' : '已添加连线',
      );
    },
    [connectionPorts, input.edges, input.setStatus, input.versionId, submit],
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

  return { commitMove, commitResize, connect, disconnect, isValidConnection };
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
