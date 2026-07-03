import type {
  ManualEditSession,
  RunConfirmationResponse,
  WorkflowGraph,
  WorkspaceMessageResponse,
  WorkbenchState,
} from './types';

export function appendChatMessages(
  state: WorkbenchState,
  messages: WorkbenchState['chat']['messages'],
): WorkbenchState {
  return {
    ...state,
    chat: {
      ...state.chat,
      messages: [...state.chat.messages, ...messages],
    },
  };
}

export function appendSystemError(state: WorkbenchState, message: string): WorkbenchState {
  return appendChatMessages(state, [
    {
      id: `msg_system_${Date.now()}`,
      role: 'system',
      kind: 'run_failed',
      text: message,
      time: messageTime(),
    },
  ]);
}

export function compatibleEditSession(
  session: ManualEditSession | null,
  state: WorkbenchState,
): ManualEditSession | null {
  return session?.baseVersionId === state.workspace.versionId ? session : null;
}

export function applyMessageResponse(
  state: WorkbenchState,
  response: WorkspaceMessageResponse,
): WorkbenchState {
  let next = appendChatMessages(state, response.messages);
  if (response.run) {
    next = applyRunSnapshot(next, response.run);
  }
  if (response.proposal) {
    next = {
      ...next,
      pendingProposal: response.proposal,
    };
  }
  if (response.pendingConfirmation !== undefined) {
    next = {
      ...next,
      pendingConfirmation: response.pendingConfirmation,
    };
  }
  return next;
}

export function applyRunConfirmation(
  state: WorkbenchState,
  response: RunConfirmationResponse,
): WorkbenchState {
  return applyRunSnapshot(
    {
      ...state,
      outputs: response.outputs,
      pendingConfirmation: response.pendingConfirmation,
    },
    response.run,
  );
}

export function markRunConfirming(state: WorkbenchState, runId: string): WorkbenchState {
  return {
    ...state,
    pendingConfirmation: null,
    run:
      state.run?.id === runId
        ? {
            ...state.run,
            status: 'running',
          }
        : state.run,
  };
}

export function applyRunSnapshot(
  state: WorkbenchState,
  run: NonNullable<WorkbenchState['run']>,
): WorkbenchState {
  const eventSeq = state.run?.id === run.id ? state.eventSeq : 0;

  return {
    ...state,
    eventSeq,
    run,
    graph: {
      ...state.graph,
      nodes: state.graph.nodes.map((node) => {
        const step = run.steps.find((candidate) => candidate.nodeId === node.id);
        return step
          ? { ...node, status: step.state, provider: step.provider, cached: step.cached }
          : node;
      }),
    },
  };
}

export function workflowGraphFromState(state: WorkbenchState): WorkflowGraph {
  if (state.workflowGraph) {
    return state.workflowGraph;
  }

  return {
    schema_version: 1,
    nodes: Object.fromEntries(
      state.graph.nodes.map((node) => [
        node.id,
        {
          node_type: node.nodeType,
          title: node.title,
          params: {},
          pos: [node.position.x, node.position.y] as [number, number],
          size: node.size ? ([node.size.width, node.size.height] as [number, number]) : undefined,
        },
      ]),
    ),
    edges: state.graph.edges.map((edge) => ({
      from: [edge.from.nodeId, edge.from.port],
      to: [edge.to.nodeId, edge.to.port],
      edge_type: edge.kind,
    })),
  };
}

export function messageTime(): string {
  return new Date().toISOString();
}
