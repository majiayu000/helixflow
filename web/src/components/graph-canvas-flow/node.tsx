import { memo } from 'react';
import { Handle, NodeResizer, Position, type NodeProps } from '@xyflow/react';
import { portColor } from '../../icons';
import { isCanvasCardType } from '../../grid-split';
import { canvasCardFallbackPorts } from '../graph-canvas-connections';
import {
  GRAPH_NODE_HEAD_HEIGHT,
  GRAPH_NODE_MAX_HEIGHT,
  GRAPH_NODE_MAX_WIDTH,
  GRAPH_NODE_ROW_HEIGHT,
} from '../graph-canvas-navigation';
import { WorkflowNode } from '../graph-canvas-node';
import type { WorkflowFlowNode } from './types';

const PORT_START_Y = GRAPH_NODE_HEAD_HEIGHT + GRAPH_NODE_ROW_HEIGHT;
const PORT_GAP = 22;

export const ReactFlowWorkflowNode = memo(function ReactFlowWorkflowNode({
  id,
  data,
  selected,
  isConnectable,
}: NodeProps<WorkflowFlowNode>) {
  const fallback = canvasCardFallbackPorts(data.node.nodeType);
  const inputs = data.definition?.inputs ?? fallback?.inputs ?? [];
  const outputs = data.definition?.outputs ?? fallback?.outputs ?? [];
  const media = isCanvasCardType(data.node.nodeType);

  return (
    <div className={media ? 'workflow-flow-node workflow-flow-node--media' : 'workflow-flow-node'}>
      {inputs.map((port, index) => (
        <Handle
          className={media ? 'workflow-flow-handle workflow-flow-handle--plus' : 'workflow-flow-handle'}
          id={port.name}
          isConnectable={isConnectable}
          key={`input:${port.name}`}
          onClick={(event) => {
            if (!media || !data.onHandleClick) return;
            event.stopPropagation();
            data.onHandleClick('target', event.clientX, event.clientY);
          }}
          position={Position.Left}
          style={{
            background: media ? undefined : portColor(port.type),
            top: media ? '50%' : PORT_START_Y + index * PORT_GAP,
          }}
          title={`${port.name} · ${port.type}`}
          type="target"
        >
          {media ? '+' : null}
        </Handle>
      ))}
      {outputs.map((port, index) => (
        <Handle
          className={media ? 'workflow-flow-handle workflow-flow-handle--plus' : 'workflow-flow-handle'}
          id={port.name}
          isConnectable={isConnectable}
          key={`output:${port.name}`}
          onClick={(event) => {
            if (!media || !data.onHandleClick) return;
            event.stopPropagation();
            data.onHandleClick('source', event.clientX, event.clientY);
          }}
          position={Position.Right}
          style={{
            background: media ? undefined : portColor(port.type),
            top: media ? '50%' : PORT_START_Y + index * PORT_GAP,
          }}
          title={`${port.name} · ${port.type}`}
          type="source"
        >
          {media ? '+' : null}
        </Handle>
      ))}
      <WorkflowNode
        artifactOutputs={data.artifactOutputs}
        connectionDisabled={!isConnectable}
        definition={data.definition}
        diffState={data.diffState}
        dirty={data.dirty}
        embedded
        locked={data.locked}
        node={data.node}
        portHighlights={EMPTY_HIGHLIGHTS}
        resizable={false}
        selected={selected}
        stepState={data.stepState}
        workflowNode={data.workflowNode}
        onKeyboardSelect={noopBoolean}
        onOutputPortPointerDown={noop}
        onPointerCancel={noop}
        onPointerDown={noop}
        onPointerMove={noop}
        onPointerUp={noop}
        onResizePointerCancel={noop}
        onResizePointerDown={noop}
        onResizePointerMove={noop}
        onResizePointerUp={noop}
        onSelectOutput={data.onSelectOutput}
        onUploadMedia={data.onUploadMedia}
        workspaceId={data.workspaceId}
      />
      {selected && data.resizable ? (
        <NodeResizer
          color={media ? 'rgb(255 255 255 / 55%)' : 'var(--accent)'}
          handleClassName="workflow-flow-resize-handle"
          keepAspectRatio={media}
          lineClassName={media ? 'workflow-flow-resize-line--hidden' : undefined}
          maxHeight={GRAPH_NODE_MAX_HEIGHT}
          maxWidth={GRAPH_NODE_MAX_WIDTH}
          minHeight={media ? 200 : 120}
          minWidth={media ? 200 : 220}
          onResizeEnd={(_event, params) => {
            data.onResizeCommit(id, params.width, params.height);
          }}
        />
      ) : null}
    </div>
  );
});

const EMPTY_HIGHLIGHTS = new Map();
const noop = () => undefined;
const noopBoolean = (_additive: boolean) => undefined;
