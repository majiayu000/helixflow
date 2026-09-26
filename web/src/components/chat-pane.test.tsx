import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import { ChatPane } from './chat-pane';
import type { WorkbenchState } from '../types';

describe('ChatPane terminal agent turns', () => {
  it('hides commit controls when there are no pending canvas edits', () => {
    const markup = renderToStaticMarkup(
      <ChatPane
        busy={false}
        messages={[]}
        onApplyProposal={async () => {}}
        onDismissProposal={async () => {}}
        onSend={async () => {}}
        pendingProposal={null}
        run={null}
      />,
    );

    expect(markup).not.toContain('UNCOMMITTED');
    expect(markup).not.toContain('提交编辑');
    expect(markup).toContain('>Chat</span>');
    expect(markup).toContain('今天一起创作点什么?');
    expect(markup).toContain('随心输入');
    expect(markup).toContain('aria-label="选择模型"');
    expect(markup).not.toContain('解锁');
    expect(markup).not.toContain('CONVERSATION');
    expect(markup).not.toContain('观察中');
  });

  it('offers a real interrupt action while the agent is busy', () => {
    const markup = renderToStaticMarkup(
      <ChatPane
        messages={[]}
        pendingProposal={null}
        run={null}
        busy
        onSend={async () => {}}
        onInterrupt={async () => {}}
        onApplyProposal={async () => {}}
        onDismissProposal={async () => {}}
      />,
    );

    expect(markup).toContain('停止 Agent');
  });

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
        onSend={async () => {}}
        onApplyProposal={async () => {}}
        onDismissProposal={async () => {}}
      />,
    );

    expect(markup).toContain('本轮执行失败 · BINDING_NOT_FOUND');
    expect(markup).not.toContain('Agent 正在执行工具调用');
  });
});
