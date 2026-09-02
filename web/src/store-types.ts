import type { ConnectionStatus } from './api';
import type {
  CanvasCommentOpInput,
  CanvasDocument,
  CanvasMessageContext,
  CanvasPresence,
  CanvasSnapshotUpdate,
  ManualEditSession,
  ManualProposalInput,
  RunEventEnvelope,
  WorkbenchState,
  WorkflowGraph,
  TurnMode,
} from './types';

export type LoadStatus = 'idle' | 'loading' | 'ready' | 'error';
export type QueueRunOptions = { forceRerun?: boolean };
export type PresenceByActor = Record<string, CanvasPresence>;

export type WorkbenchStore = {
  status: LoadStatus;
  error: string | null;
  connection: ConnectionStatus;
  canvasStatus: LoadStatus;
  canvasError: string | null;
  canvasConnection: ConnectionStatus;
  canvas: CanvasDocument | null;
  selectedCanvasNodeIds: string[];
  presenceByActor: PresenceByActor;
  state: WorkbenchState | null;
  editSession: ManualEditSession | null;
  /** Last seen event seq per run/agent stream (HF-012). */
  streamSeqs: Record<string, number>;
  workspaceGeneration: number;
  activeWorkspaceId: string | null;
  bootstrap: (workspaceId?: string | null) => Promise<void>;
  hydrate: (workspaceId: string) => Promise<void>;
  createWorkspace: () => Promise<void>;
  setInitialState: (state: WorkbenchState) => void;
  setConnection: (status: ConnectionStatus) => void;
  setCanvasSelection: (nodeIds: string[]) => void;
  applyCanvasPresence: (presence: CanvasPresence) => void;
  sendCanvasPresence: (presence: CanvasPresence) => Promise<void>;
  submitCanvasCommentOp: (input: CanvasCommentOpInput) => Promise<void>;
  applyEvent: (event: RunEventEnvelope, generation?: number) => void;
  sendMessage: (
    text: string,
    canvasContext?: CanvasMessageContext,
    conversationId?: string,
    turnMode?: TurnMode,
  ) => Promise<void>;
  createConversation: () => Promise<string>;
  uploadImage: (file: File) => Promise<void>;
  queueRun: (options?: QueueRunOptions) => Promise<void>;
  interruptRun: (runId?: string) => Promise<void>;
  interruptAgent: () => Promise<void>;
  exportWorkflow: () => Promise<WorkflowGraph | null>;
  undoVersion: () => Promise<void>;
  restoreVersion: (versionId: string) => Promise<void>;
  saveCanvasSnapshot: (update: CanvasSnapshotUpdate) => Promise<void>;
  selectOutput: (outputId: string) => Promise<void>;
  acceptOutput: (outputId: string) => Promise<void>;
  rejectOutput: (outputId: string, rerun?: boolean) => Promise<void>;
  selectProvider: (providerId: string) => Promise<void>;
  appendManualEdit: (input: ManualProposalInput) => Promise<void>;
  commitManualEdits: () => Promise<void>;
  discardManualEdits: () => void;
  confirmRun: (runId: string) => Promise<void>;
  holdRun: (runId: string) => Promise<void>;
  createManualProposal: (input: ManualProposalInput) => Promise<void>;
  applyProposal: (proposalId: string) => Promise<void>;
  dismissProposal: (proposalId: string) => Promise<void>;
};
