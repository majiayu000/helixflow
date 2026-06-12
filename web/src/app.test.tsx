import { renderToStaticMarkup } from 'react-dom/server';
import { beforeEach, describe, expect, it } from 'vitest';
import { App } from './app';
import { applyRunEvent, useWorkbenchStore } from './store';
import type { WorkbenchState } from './types';

const state: WorkbenchState = {
  eventSeq: 0,
  workspace: {
    id: 'demo',
    name: 'Helixflow Demo',
    versionId: 'ver_demo_1',
    updatedAt: '2026-06-12T00:00:00Z',
  },
  chat: {
    messages: [
      {
        id: 'msg_1',
        role: 'user',
        text: 'Create a vertical product teaser.',
        time: '09:10',
      },
    ],
  },
  graph: {
    nodes: [
      {
        id: 'text',
        nodeType: 'input.text',
        title: 'Launch note',
        category: 'Input',
        status: 'succeeded',
        position: { x: 48, y: 158 },
        provider: null,
        summary: 'Source copy',
      },
      {
        id: 'video',
        nodeType: 'video.mock.text_to_video',
        title: 'Video render',
        category: 'Video',
        status: 'queued',
        position: { x: 486, y: 156 },
        provider: 'mock',
        summary: '9:16, 4 seconds',
      },
    ],
    edges: [
      {
        id: 'edge_text_video',
        from: { nodeId: 'text', port: 'text' },
        to: { nodeId: 'video', port: 'prompt' },
        kind: 'text',
      },
    ],
  },
  run: {
    id: 'run_demo_1',
    label: 'Manual preview',
    status: 'running',
    steps: [
      { nodeId: 'text', title: 'Launch note', state: 'succeeded', provider: null },
      { nodeId: 'video', title: 'Video render', state: 'queued', provider: 'mock' },
    ],
    cost: { estimate: 0, actual: 0, currency: 'USD' },
  },
  outputs: [
    {
      id: 'art_video_1',
      kind: 'video',
      title: 'Vertical teaser',
      storageUri: 'workspace://outputs/run_demo_1/video/text_to_video.mp4',
      selected: true,
      meta: '1080 x 1920',
    },
  ],
  history: [
    {
      id: 'hist_1',
      kind: 'version',
      label: 'Initial graph',
      time: '09:05',
      summary: 'Version ver_demo_1',
    },
  ],
  pendingConfirmation: {
    id: 'confirm_1',
    title: 'Agent requested run',
    summary: 'Run the current graph.',
    cost: { amount: 0, currency: 'USD' },
  },
};

describe('App', () => {
  beforeEach(() => {
    useWorkbenchStore.setState({
      status: 'idle',
      error: null,
      connection: 'offline',
      state: null,
    });
  });

  it('renders the primary workbench layout from backend state', () => {
    const markup = renderToStaticMarkup(<App initialState={state} />);

    expect(markup).toContain('Helixflow Demo');
    expect(markup).toContain('Chat');
    expect(markup).toContain('Canvas');
    expect(markup).toContain('Run');
    expect(markup).toContain('Outputs');
    expect(markup).toContain('History');
    expect(markup).toContain('Agent requested run');
  });

  it('applies websocket node state events to visible run state', () => {
    const updated = applyRunEvent(state, {
      workspace_id: 'demo',
      run_id: 'run_demo_1',
      seq: 8,
      server_time: '2026-06-12T00:00:01Z',
      ev: 'node.state',
      data: { node_id: 'video', state: 'running' },
    });

    expect(updated.eventSeq).toBe(8);
    expect(updated.graph.nodes.find((node) => node.id === 'video')?.status).toBe('running');
    expect(updated.run.steps.find((step) => step.nodeId === 'video')?.state).toBe('running');
  });

  it('ignores websocket events for another run in the same workspace', () => {
    const updated = applyRunEvent(state, {
      workspace_id: 'demo',
      run_id: 'run_other',
      seq: 8,
      server_time: '2026-06-12T00:00:01Z',
      ev: 'node.state',
      data: { node_id: 'video', state: 'running' },
    });

    expect(updated).toBe(state);
  });

  it('applies agent status events even when they use an agent session id', () => {
    const updated = applyRunEvent(state, {
      workspace_id: 'demo',
      run_id: 'agent_session_1',
      seq: 1,
      server_time: '2026-06-12T00:00:01Z',
      ev: 'agent.status',
      data: {
        session_id: 'agent_session_1',
        status: 'runtime.status',
        detail: { message: 'Drafting graph proposal' },
      },
    });

    expect(updated.eventSeq).toBe(1);
    expect(updated.chat.messages.at(-1)).toMatchObject({
      id: 'agent-status-agent_session_1',
      role: 'agent',
      text: 'Drafting graph proposal',
    });
  });
});
