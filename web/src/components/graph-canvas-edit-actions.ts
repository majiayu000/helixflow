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
  buildPasteProposalInput,
  buildSelectionClipboardText,
} from './graph-canvas-editing';
import type { Point } from './graph-canvas-selection';

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

  const addNode = (definition: NodeDefinition, position = viewportCenterWorld()) => {
    if (!input.capabilities.paste) return;
    submit(
      buildAddNodeProposalInput({
        baseVersionId: input.versionId,
        definition,
        existingNodeIds: input.drawGraph.nodes.map((node) => node.id),
        position,
      }),
      `已加入编辑会话：添加 ${definition.title}`,
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
    if (!input.capabilities.paste) return;
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
      submit(proposal, '已加入编辑会话：粘贴子图', '粘贴失败');
    } catch (error) {
      input.setClipboardStatus(error instanceof Error ? `粘贴失败：${error.message}` : '粘贴失败');
    }
  };

  const deleteSelection = (selectedNodeIds: Iterable<string>) => {
    if (!input.capabilities.delete) return;
    const ids = [...selectedNodeIds];
    submit(
      buildDeleteNodesProposalInput({ baseVersionId: input.versionId, selectedNodeIds: ids }),
      `已加入编辑会话：删除 ${ids.length} 个节点`,
      '删除节点失败',
    );
  };

  return { addNode, copySelection, deleteSelection, pasteSelection };
}
