import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import { ChatPane } from './chat-pane';
import type { WorkbenchState } from '../types';

describe('ChatPane terminal agent turns', () => {
  it('renders a persisted log-only failure as terminal instead of running forever', () => {
    const messages: WorkbenchState['chat']['messages'] = [
      {
        id: 'msg_user',
        role: 'user',
        kind: 'text',
        text: '创建 workflow',
        time: '2026-08-10T10:32:00Z',
        conversationId: 'conv_1',
        turnId: 'turn_1',
      },
      {
        id: 'msg_log',
        role: 'agent',
        kind: 'agent_log:status',
        text: 'tool call completed',
        time: '2026-08-10T10:33:00Z',
        conversationId: 'conv_1',
        turnId: 'turn_1',
      },
    ];
    const turns: NonNullable<WorkbenchState['chat']['turns']> = [
      {
        id: 'turn_1',
        conversationId: 'conv_1',
        mode: 'create_workflow',
        status: 'error',
        reasonCode: 'BINDING_NOT_FOUND',
        startedAt: '2026-08-10T10:32:00Z',
        completedAt: '2026-08-10T10:33:01Z',
      },
    ];

    const markup = renderToStaticMarkup(
      <ChatPane
        messages={messages}
        turns={turns}
        pendingProposal={null}
        run={null}
        busy={false}
        editSessionSummary={null}
        onSend={async () => {}}
        onCommitEdits={async () => {}}
        onDiscardEdits={() => {}}
        onApplyProposal={async () => {}}
        onDismissProposal={async () => {}}
      />,
    );

    expect(markup).toContain('本轮执行失败 · BINDING_NOT_FOUND');
    expect(markup).not.toContain('Agent 正在执行工具调用');
  });
});
