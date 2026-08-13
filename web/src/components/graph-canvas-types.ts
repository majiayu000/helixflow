import type {
  CanvasComment,
  CanvasCommentOpInput,
  CanvasPresence,
  ManualProposalInput,
  WorkbenchState,
} from '../types';
import type { ConnectionPort } from './graph-canvas-connections';
import type { DragNodeStart } from './graph-canvas-layout';
import type { Point } from './graph-canvas-selection';

export type GraphCanvasProps = {
  workspaceId: string;
  versionId: string;
  graph: WorkbenchState['graph'];
  canvasGraph?: WorkbenchState['graph'];
  comments?: CanvasComment[];
  pendingProposal: WorkbenchState['pendingProposal'];
  presenceByActor?: Record<string, CanvasPresence>;
  run: NonNullable<WorkbenchState['run']>;
  workflowGraph?: WorkbenchState['workflowGraph'];
  onCommentOp?: (input: CanvasCommentOpInput) => Promise<void>;
  onCreateProposal?: (input: ManualProposalInput) => Promise<void>;
  onPresenceChange?: (presence: CanvasPresence) => void | Promise<void>;
  onRequestNodeProposal?: (nodeId: string) => Promise<void>;
  onSelectOutput?: (outputId: string) => void;
  onSelectionChange?: (nodeIds: string[]) => void;
  onSetParam?: (nodeId: string, key: string, value: unknown) => Promise<void>;
  onStartFromPrompt?: (prompt: string) => void | Promise<void>;
  outputs?: WorkbenchState['outputs'];
};

export type DragState = {
  pointerId: number;
  sx: number;
  sy: number;
  ox: number;
  oy: number;
};

export type NodeDragState = {
  pointerId: number;
  sx: number;
  sy: number;
  starts: DragNodeStart[];
};

export type SelectionDragState = {
  pointerId: number;
  start: Point;
  current: Point;
  additive: boolean;
  baseIds: Set<string>;
};

export type ConnectionDragState = {
  pointerId: number;
  source: ConnectionPort;
  sourcePoint: Point;
  currentPoint: Point;
};
