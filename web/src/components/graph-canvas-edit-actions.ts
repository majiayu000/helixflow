import type {
  GraphNodeState,
  ManualProposalInput,
  NodeDefinition,
  WorkbenchState,
  WorkflowGraph,
} from '../types';
import type { ViewState, ViewportSize } from './graph-canvas-navigation';
import type { CanvasCapabilities } from './graph-canvas-capabilities';
import {
  buildAddNodeProposalInput,
  buildDeleteNodesProposalInput,
  buildDuplicateNodeProposalInput,
  buildPasteProposalInput,
  buildSelectionClipboardText,
} from './graph-canvas-editing';
import type { Point } from './graph-canvas-selection';
import {
  GRAPH_NODE_MIN_HEIGHT,
  GRAPH_NODE_WIDTH,
  graphNodeHeight,
  graphNodeWidth,
} from './graph-canvas-navigation';

type CanvasEditActionsInput = {
  capabilities: CanvasCapabilities;
  drawGraph: WorkbenchState['graph'];
  onCreateProposal?: (input: ManualProposalInput) => Promise<void>;
  setClipboardStatus: (value: string | null) => void;
  versionId: string;
  view: ViewState;
  viewportSize: ViewportSize;
  workflowGraph?: WorkflowGraph;
};

export function createCanvasEditActions(input: CanvasEditActionsInput) {
  const viewportCenterWorld = (): Point => ({
    x: (input.viewportSize.width / 2 - input.view.x) / input.view.z,
    y: (input.viewportSize.height / 2 - input.view.y) / input.view.z,
  });

  const submit = (proposal: ManualProposalInput | null, success: string, failure: string) => {
    if (!proposal || !input.onCreateProposal) return;
    void input.onCreateProposal(proposal)
      .then(() => input.setClipboardStatus(success))
      .catch((error) => {
        input.setClipboardStatus(error instanceof Error ? error.message : failure);
      });
  };

  const addNode = (
    definition: NodeDefinition,
    position?: Point,
    connectFrom?: { nodeId: string; definition: NodeDefinition; direction?: 'in' | 'out' },
  ) => {
    if (!input.capabilities.paste) {
      input.setClipboardStatus('当前模式不允许添加节点');
      return;
    }
    submit(
      buildAddNodeProposalInput({
        baseVersionId: input.versionId,
        definition,
        existingNodeIds: input.drawGraph.nodes.map((node) => node.id),
        position: position ?? nextAvailableNodePosition(viewportCenterWorld(), input.drawGraph.nodes),
        connectFrom,
      }),
      `已添加 ${definition.title}`,
      '添加节点失败',
    );
  };

  const copySelection = async (nodes: GraphNodeState[]) => {
    if (nodes.length === 0) return;
    if (!navigator.clipboard?.writeText) {
      input.setClipboardStatus('复制失败：浏览器不支持 clipboard');
      return;
    }
    try {
      await navigator.clipboard.writeText(buildSelectionClipboardText({
        sourceVersionId: input.versionId,
        nodes,
        edges: input.drawGraph.edges,
        workflowGraph: input.workflowGraph,
      }));
      input.setClipboardStatus(`已复制 ${nodes.length} 个节点`);
    } catch (error) {
      input.setClipboardStatus(error instanceof Error ? `复制失败：${error.message}` : '复制失败');
    }
  };

  const pasteSelection = async () => {
    if (!input.capabilities.paste) {
      input.setClipboardStatus('当前模式不允许粘贴');
      return;
    }
    if (!navigator.clipboard?.readText) {
      input.setClipboardStatus('粘贴失败：浏览器不支持 clipboard');
      return;
    }
    try {
      const proposal = buildPasteProposalInput({
        baseVersionId: input.versionId,
        existingNodeIds: input.drawGraph.nodes.map((node) => node.id),
        text: await navigator.clipboard.readText(),
        position: viewportCenterWorld(),
      });
      if (!proposal) {
        input.setClipboardStatus('粘贴失败：无效选区');
        return;
      }
      submit(proposal, '已粘贴子图', '粘贴失败');
    } catch (error) {
      input.setClipboardStatus(error instanceof Error ? `粘贴失败：${error.message}` : '粘贴失败');
    }
  };

  const duplicateNode = (node: GraphNodeState) => {
    if (!input.capabilities.paste) {
      input.setClipboardStatus('当前模式不允许添加节点');
      return;
    }
    submit(
      buildDuplicateNodeProposalInput({
        baseVersionId: input.versionId,
        existingNodeIds: input.drawGraph.nodes.map((item) => item.id),
        source: node,
        params: input.workflowGraph?.nodes[node.id]?.params,
      }),
      `已复制 ${node.title}`,
      '复制节点失败',
    );
  };

  const deleteSelection = (selectedNodeIds: Iterable<string>) => {
    if (!input.capabilities.delete) {
      input.setClipboardStatus('当前模式不允许删除');
      return;
    }
    const ids = [...selectedNodeIds];
    submit(
      buildDeleteNodesProposalInput({ baseVersionId: input.versionId, selectedNodeIds: ids }),
      `已删除 ${ids.length} 个节点`,
      '删除节点失败',
    );
  };

  return { addNode, copySelection, deleteSelection, duplicateNode, pasteSelection };
}

const NODE_GAP = 24;

/** Places library-added nodes near the viewport center without stacking them. */
export function nextAvailableNodePosition(
  origin: Point,
  existingNodes: GraphNodeState[],
): Point {
  const stepX = GRAPH_NODE_WIDTH + NODE_GAP;
  const stepY = GRAPH_NODE_MIN_HEIGHT + NODE_GAP;
  const offsets: Point[] = [{ x: 0, y: 0 }];
  for (let ring = 1; ring <= 12; ring += 1) {
    for (let x = -ring; x <= ring; x += 1) {
      offsets.push({ x: x * stepX, y: -ring * stepY });
      offsets.push({ x: x * stepX, y: ring * stepY });
    }
    for (let y = -ring + 1; y < ring; y += 1) {
      offsets.push({ x: -ring * stepX, y: y * stepY });
      offsets.push({ x: ring * stepX, y: y * stepY });
    }
  }

  return offsets
    .map((offset) => ({ x: origin.x + offset.x, y: origin.y + offset.y }))
    .find((candidate) => existingNodes.every((node) => !overlapsNode(candidate, node)))
    ?? origin;
}

function overlapsNode(candidate: Point, node: GraphNodeState): boolean {
  const candidateRight = candidate.x + GRAPH_NODE_WIDTH;
  const candidateBottom = candidate.y + GRAPH_NODE_MIN_HEIGHT;
  const nodeRight = node.position.x + graphNodeWidth(node);
  const nodeBottom = node.position.y + graphNodeHeight(node);
  return !(
    candidateRight + NODE_GAP <= node.position.x
    || candidate.x >= nodeRight + NODE_GAP
    || candidateBottom + NODE_GAP <= node.position.y
    || candidate.y >= nodeBottom + NODE_GAP
  );
}
