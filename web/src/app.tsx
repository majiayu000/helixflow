import { useEffect } from 'react';
import { connectWorkspaceEvents } from './api';
import { useWorkbenchStore } from './store';
import type { GraphNodeState, RunStepState, WorkbenchState } from './types';

const DEFAULT_WORKSPACE_ID = 'demo';

type AppProps = {
  initialState?: WorkbenchState;
};

export function App({ initialState }: AppProps) {
  const status = useWorkbenchStore((store) => store.status);
  const error = useWorkbenchStore((store) => store.error);
  const state = useWorkbenchStore((store) => store.state);
  const connection = useWorkbenchStore((store) => store.connection);
  const hydrate = useWorkbenchStore((store) => store.hydrate);
  const setInitialState = useWorkbenchStore((store) => store.setInitialState);
  const setConnection = useWorkbenchStore((store) => store.setConnection);
  const applyEvent = useWorkbenchStore((store) => store.applyEvent);
  const activeState = initialState ?? state;

  useEffect(() => {
    if (initialState) {
      setInitialState(initialState);
      return;
    }
    if (status === 'idle') {
      void hydrate(DEFAULT_WORKSPACE_ID);
    }
  }, [hydrate, initialState, setInitialState, status]);

  useEffect(() => {
    if (!activeState) {
      return undefined;
    }

    return connectWorkspaceEvents(activeState.workspace.id, {
      onEvent: applyEvent,
      onStatus: setConnection,
    });
  }, [activeState?.workspace.id, applyEvent, setConnection]);

  if (!activeState) {
    return (
      <main className="loading-shell">
        <section className="loading-panel">
          <p>Helixflow</p>
          <h1>{status === 'error' ? 'Workspace unavailable' : 'Loading workspace'}</h1>
          {error ? <span>{error}</span> : <span>127.0.0.1:8787</span>}
        </section>
      </main>
    );
  }

  return (
    <main className="workbench-shell">
      <TopBar state={activeState} connection={connection} />
      <section className="workbench-grid" aria-label="Helixflow workbench">
        <ChatPane state={activeState} />
        <GraphCanvas state={activeState} />
        <RightRail state={activeState} />
      </section>
      {activeState.pendingConfirmation ? (
        <ConfirmModal confirmation={activeState.pendingConfirmation} />
      ) : null}
    </main>
  );
}

function TopBar({
  state,
  connection,
}: {
  state: WorkbenchState;
  connection: string;
}) {
  return (
    <header className="top-bar">
      <div>
        <p className="product-mark">Helixflow</p>
        <h1>{state.workspace.name}</h1>
      </div>
      <div className="top-meta" aria-label="Workspace status">
        <span>{state.workspace.versionId}</span>
        <span>{state.run.status}</span>
        <span>{connection}</span>
      </div>
      <button className="primary-action" type="button">
        Run
      </button>
    </header>
  );
}

function ChatPane({ state }: { state: WorkbenchState }) {
  return (
    <aside className="panel chat-pane" aria-label="Chat">
      <div className="panel-heading">
        <h2>Chat</h2>
        <span>{state.chat.messages.length}</span>
      </div>
      <div className="message-list">
        {state.chat.messages.map((message) => (
          <article className="message-row" data-role={message.role} key={message.id}>
            <span>{message.role}</span>
            <p>{message.text}</p>
            <time>{message.time}</time>
          </article>
        ))}
      </div>
      <form className="composer" aria-label="Message composer">
        <input aria-label="Message" placeholder="Ask for a graph change" />
        <button type="button">Send</button>
      </form>
    </aside>
  );
}

function GraphCanvas({ state }: { state: WorkbenchState }) {
  return (
    <section className="panel graph-panel" aria-label="Canvas">
      <div className="panel-heading">
        <h2>Canvas</h2>
        <span>{state.graph.nodes.length} nodes</span>
      </div>
      <div className="canvas-board">
        <svg className="edge-layer" viewBox="0 0 860 430" aria-hidden="true">
          {state.graph.edges.map((edge) => {
            const from = state.graph.nodes.find((node) => node.id === edge.from.nodeId);
            const to = state.graph.nodes.find((node) => node.id === edge.to.nodeId);
            if (!from || !to) {
              return null;
            }
            return (
              <line
                key={edge.id}
                x1={from.position.x + 156}
                y1={from.position.y + 34}
                x2={to.position.x}
                y2={to.position.y + 34}
              />
            );
          })}
        </svg>
        {state.graph.nodes.map((node) => (
          <GraphNode key={node.id} node={node} />
        ))}
      </div>
    </section>
  );
}

function GraphNode({ node }: { node: GraphNodeState }) {
  return (
    <article
      className="graph-node"
      data-state={node.status}
      style={{ left: node.position.x, top: node.position.y }}
    >
      <div>
        <span>{node.category}</span>
        <strong>{node.title}</strong>
      </div>
      <p>{node.summary}</p>
      <footer>
        <span>{node.nodeType}</span>
        <StatusPill state={node.status} />
      </footer>
    </article>
  );
}

function RightRail({ state }: { state: WorkbenchState }) {
  return (
    <aside className="right-rail" aria-label="Run details">
      <RunDock state={state} />
      <OutputsStrip state={state} />
      <HistoryPanel state={state} />
    </aside>
  );
}

function RunDock({ state }: { state: WorkbenchState }) {
  return (
    <section className="panel run-dock" aria-label="Run dock">
      <div className="panel-heading">
        <h2>Run</h2>
        <StatusPill state={state.run.status} />
      </div>
      <div className="run-summary">
        <strong>{state.run.label}</strong>
        <span>
          {state.run.cost.actual.toFixed(2)} {state.run.cost.currency}
        </span>
      </div>
      <ol className="step-list">
        {state.run.steps.map((step) => (
          <li key={step.nodeId} data-state={step.state}>
            <span>{step.title}</span>
            <StatusPill state={step.state} />
          </li>
        ))}
      </ol>
    </section>
  );
}

function OutputsStrip({ state }: { state: WorkbenchState }) {
  return (
    <section className="panel outputs-strip" aria-label="Outputs">
      <div className="panel-heading">
        <h2>Outputs</h2>
        <span>{state.outputs.length}</span>
      </div>
      <div className="output-list">
        {state.outputs.map((output) => (
          <article key={output.id} data-selected={output.selected}>
            <span>{output.kind}</span>
            <strong>{output.title}</strong>
            <p>{output.meta}</p>
          </article>
        ))}
      </div>
    </section>
  );
}

function HistoryPanel({ state }: { state: WorkbenchState }) {
  return (
    <section className="panel history-panel" aria-label="History">
      <div className="panel-heading">
        <h2>History</h2>
        <span>{state.history.length}</span>
      </div>
      <div className="history-list">
        {state.history.map((item) => (
          <article key={item.id}>
            <span>{item.kind}</span>
            <strong>{item.label}</strong>
            <time>{item.time}</time>
          </article>
        ))}
      </div>
    </section>
  );
}

function ConfirmModal({
  confirmation,
}: {
  confirmation: NonNullable<WorkbenchState['pendingConfirmation']>;
}) {
  return (
    <div className="modal-layer">
      <section className="confirm-modal" role="dialog" aria-modal="true" aria-label="Confirm run">
        <div>
          <span>Confirm</span>
          <h2>{confirmation.title}</h2>
          <p>{confirmation.summary}</p>
        </div>
        <strong>
          {confirmation.cost.amount.toFixed(2)} {confirmation.cost.currency}
        </strong>
        <div className="modal-actions">
          <button type="button">Hold</button>
          <button type="button">Approve</button>
        </div>
      </section>
    </div>
  );
}

function StatusPill({ state }: { state: RunStepState | WorkbenchState['run']['status'] }) {
  return <span className="status-pill" data-state={state}>{state}</span>;
}
