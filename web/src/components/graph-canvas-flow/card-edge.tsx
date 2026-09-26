import {
  BaseEdge,
  getBezierPath,
  Position,
  useInternalNode,
  type ConnectionLineComponentProps,
  type EdgeProps,
} from '@xyflow/react';
import { isCanvasCardType } from '../../grid-split';
import { internalNodeBox, mediaCardAnchor } from './card-edge-geometry';
import type { WorkflowFlowEdge, WorkflowFlowNode } from './types';

export function CardEdge({
  id,
  source,
  target,
  sourceX,
  sourceY,
  targetX,
  targetY,
  sourcePosition,
  targetPosition,
  style,
  markerEnd,
}: EdgeProps<WorkflowFlowEdge>) {
  const sourceNode = useInternalNode<WorkflowFlowNode>(source);
  const targetNode = useInternalNode<WorkflowFlowNode>(target);
  const sourceBox =
    sourceNode && isCanvasCardType(sourceNode.data.node.nodeType)
      ? internalNodeBox(sourceNode)
      : null;
  const targetBox =
    targetNode && isCanvasCardType(targetNode.data.node.nodeType)
      ? internalNodeBox(targetNode)
      : null;
  const from = sourceBox ? mediaCardAnchor(sourceBox, 'right') : { x: sourceX, y: sourceY };
  const to = targetBox ? mediaCardAnchor(targetBox, 'left') : { x: targetX, y: targetY };
  const [path] = getBezierPath({
    sourceX: from.x,
    sourceY: from.y,
    targetX: to.x,
    targetY: to.y,
    sourcePosition: sourceBox ? Position.Right : sourcePosition,
    targetPosition: targetBox ? Position.Left : targetPosition,
  });
  return <BaseEdge id={id} markerEnd={markerEnd} path={path} style={style} />;
}

export function CardConnectionLine({
  fromNode,
  fromHandle,
  fromX,
  fromY,
  fromPosition,
  toNode,
  toHandle,
  toX,
  toY,
  toPosition,
  connectionLineStyle,
}: ConnectionLineComponentProps<WorkflowFlowNode>) {
  const sourceBox = isCanvasCardType(fromNode.data.node.nodeType) ? internalNodeBox(fromNode) : null;
  const targetBox =
    toNode && isCanvasCardType(toNode.data.node.nodeType) ? internalNodeBox(toNode) : null;
  const from = sourceBox
    ? mediaCardAnchor(sourceBox, fromHandle.type === 'target' ? 'left' : 'right')
    : { x: fromX, y: fromY };
  const to = targetBox
    ? mediaCardAnchor(targetBox, toHandle?.type === 'source' ? 'right' : 'left')
    : { x: toX, y: toY };
  const [path] = getBezierPath({
    sourceX: from.x,
    sourceY: from.y,
    targetX: to.x,
    targetY: to.y,
    sourcePosition: sourceBox
      ? fromHandle.type === 'target'
        ? Position.Left
        : Position.Right
      : fromPosition,
    targetPosition: targetBox
      ? toHandle?.type === 'source'
        ? Position.Right
        : Position.Left
      : toPosition,
  });
  return (
    <path
      className="react-flow__connection-path"
      d={path}
      fill="none"
      style={connectionLineStyle}
    />
  );
}
