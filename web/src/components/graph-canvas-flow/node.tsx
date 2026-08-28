import { memo } from 'react';
import { Handle, NodeResizer, Position, type NodeProps } from '@xyflow/react';
import { portColor } from '../../icons';
import {
  GRAPH_NODE_HEAD_HEIGHT,
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
  const inputs = data.definition?.inputs ?? [];
  const outputs = data.definition?.outputs ?? [];

  return (
    <div className="workflow-flow-node">
      {selected && data.resizable ? (
        <NodeResizer
          color="var(--accent)"
          minHeight={120}
          minWidth={220}
          onResizeEnd={(_event, params) => {
            data.onResizeCommit(id, params.width, params.height);
          }}
        />
      ) : null}
      {inputs.map((port, index) => (
        <Handle
          className="workflow-flow-handle"
          id={port.name}
          isConnectable={isConnectable}
          key={`input:${port.name}`}
          position={Position.Left}
          style={{
            background: portColor(port.type),
            top: PORT_START_Y + index * PORT_GAP,
          }}
          title={`${port.name} · ${port.type}`}
          type="target"
        />
      ))}
      {outputs.map((port, index) => (
        <Handle
          className="workflow-flow-handle"
          id={port.name}
          isConnectable={isConnectable}
          key={`output:${port.name}`}
          position={Position.Right}
          style={{
            background: portColor(port.type),
            top: PORT_START_Y + index * PORT_GAP,
          }}
          title={`${port.name} · ${port.type}`}
          type="source"
        />
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
      />
    </div>
  );
});

const EMPTY_HIGHLIGHTS = new Map();
const noop = () => undefined;
const noopBoolean = (_additive: boolean) => undefined;
