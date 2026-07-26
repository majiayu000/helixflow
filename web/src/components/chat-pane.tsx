import { useEffect, useRef, useState } from 'react';
import { Icon } from '../icons';
import type { WorkbenchState } from '../types';

type ChatMessage = WorkbenchState['chat']['messages'][number];
type PendingProposal = NonNullable<WorkbenchState['pendingProposal']>;
type ChatEntry =
  | { type: 'message'; message: ChatMessage }
  | { type: 'assistantTurn'; id: string; message?: ChatMessage; logs: ChatMessage[] };
type EditSessionSummary = {
  baseVersionId: string;
  count: number;
  items: string[];
};
type ComposerKeyEvent = {
  key: string;
  shiftKey: boolean;
  nativeEvent: {
    isComposing?: boolean;
    keyCode?: number;
    which?: number;
  };
};

type ChatPaneProps = {
  messages: ChatMessage[];
  pendingProposal: WorkbenchState['pendingProposal'];
  run: WorkbenchState['run'];
  busy: boolean;
  editSessionSummary: EditSessionSummary | null;
  selectedNodeIds?: string[];
  onSend: (text: string) => Promise<void>;
  onUploadImage?: (file: File) => Promise<void>;
  onCommitEdits: () => Promise<void>;
  onDiscardEdits: () => void;
  onApplyProposal: (proposalId: string) => Promise<void>;
  onDismissProposal: (proposalId: string) => Promise<void>;
};

export function ChatPane({
  messages,
  pendingProposal,
  run,
  busy,
  editSessionSummary,
  selectedNodeIds = [],
  onSend,
  onUploadImage,
  onCommitEdits,
  onDiscardEdits,
  onApplyProposal,
  onDismissProposal,
}: ChatPaneProps) {
  const [draft, setDraft] = useState('');
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const uploadInputRef = useRef<HTMLInputElement | null>(null);
  const isComposingRef = useRef(false);
  const selectedSummary = selectedNodeIds.length > 0
    ? `@选中 ${selectedNodeIds.join(', ')}`
    : '@未选中';

  useEffect(() => {
    const element = scrollRef.current;
    if (element) {
      element.scrollTop = element.scrollHeight;
    }
  }, [messages]);

  const submit = async (text = draft) => {
    const trimmed = text.trim();
    if (!trimmed || busy) return;
    const submittedDraft = draft;
    const nextDraft = await composerDraftAfterSubmit(submittedDraft, trimmed, onSend);
    setDraft((current) => (current === submittedDraft ? nextDraft : current));
  };

  return (
    <aside className="wb-chat">
      <div className="chat-head">
        <span>SESSION · CODEX</span>
        <span className="sub">{busy ? 'busy · 执行中' : 'idle · 观察中'}</span>
      </div>
      <div className="session-brief">
        你在手动改图 — 我不会打断。提交编辑后，我的下一个提案会基于它。
      </div>
      <div className="chat-msgs chat-msgs--session" ref={scrollRef}>
        <EditSessionWorkspace
          busy={busy}
          selectedNodeIds={selectedNodeIds}
          summary={editSessionSummary}
          onCommit={onCommitEdits}
          onDiscard={onDiscardEdits}
        />
        <MessageTimeline messages={messages} />
        <RunErrorCard busy={busy} run={run} onRequestFix={onSend} />
        {pendingProposal && <ProposalMessage
          busy={busy}
          onApply={() => onApplyProposal(pendingProposal.id)}
          onDismiss={() => onDismissProposal(pendingProposal.id)}
          proposal={pendingProposal}
        />}
      </div>
      <div className="composer">
        <div className="composer-context">
          <span>节点对话</span>
          <span>{selectedSummary}</span>
          {selectedNodeIds.length > 1 && <span>含上游 · {selectedNodeIds.length} 个节点</span>}
        </div>
        <div className="composer-box">
          <textarea
            aria-label="message composer"
            disabled={busy}
            onChange={(event) => setDraft(event.target.value)}
            onCompositionEnd={() => {
              isComposingRef.current = false;
            }}
            onCompositionStart={() => {
              isComposingRef.current = true;
            }}
            onKeyDown={(event) => {
              if (shouldSubmitComposerKey(event, isComposingRef.current)) {
                event.preventDefault();
                void submit();
              }
            }}
            placeholder={selectedNodeIds.length > 0 ? '围绕选中节点提问...' : '围绕当前工作流提问...'}
            rows={2}
            value={draft}
          />
          <div className="composer-foot">
            <div className="left">
              <input
                accept="image/*"
                aria-label="上传图片"
                onChange={(event) => {
                  const file = event.target.files?.[0];
                  event.target.value = '';
                  if (file && onUploadImage) void onUploadImage(file);
                }}
                ref={uploadInputRef}
                style={{ display: 'none' }}
                type="file"
              />
              <button
                className="ibtn ibtn--icon"
                disabled={busy || !onUploadImage}
                onClick={() => uploadInputRef.current?.click()}
                title="上传图片"
              >
                <Icon n="paperclip" s={14} />
              </button>
            </div>
            <div className="right">
              <span className="kbd">Enter 发送 · Shift+Enter 换行</span>
              <button className="btn btn--primary btn--sm" disabled={busy} onClick={() => submit()}>
                <Icon n="arrowUp" s={13} />
              </button>
            </div>
          </div>
        </div>
      </div>
    </aside>
  );
}

export async function composerDraftAfterSubmit(
  draft: string,
  submittedText: string,
  onSend: (text: string) => Promise<void>,
): Promise<string> {
  try {
    await onSend(submittedText);
    return submittedText === draft.trim() ? '' : draft;
  } catch {
    return draft;
  }
}

function EditSessionWorkspace({
  summary,
  selectedNodeIds,
  busy,
  onCommit,
  onDiscard,
}: {
  summary: EditSessionSummary | null;
  selectedNodeIds: string[];
  busy: boolean;
  onCommit: () => Promise<void>;
  onDiscard: () => void;
}) {
  return (
    <div className="edit-session-workspace">
      {summary ? (
        <EditSessionCard
          busy={busy}
          summary={summary}
          onCommit={onCommit}
          onDiscard={onDiscard}
        />
      ) : (
        <EditSessionIdleCard />
      )}
      <div className="edit-session-request">
        围绕当前画布继续编辑；我会在你提交后再基于新版本工作。
      </div>
      <div className="edit-session-note">
        好 — 会围绕
        <span>{selectedNodeIds.length > 0 ? selectedNodeIds.join(' + ') : '当前工作流'}</span>
        的内容改写，结果回填到节点对话框。
      </div>
      <div className="edit-session-chips">
        <span>@选中 {selectedNodeIds.length > 0 ? selectedNodeIds.join(', ') : '无'}</span>
        <span>含上游 ×{Math.max(1, selectedNodeIds.length || 1)}</span>
      </div>
    </div>
  );
}

function EditSessionCard({
  summary,
  busy,
  onCommit,
  onDiscard,
}: {
  summary: EditSessionSummary;
  busy: boolean;
  onCommit: () => Promise<void>;
  onDiscard: () => void;
}) {
  return (
    <div className="edit-session-card">
      <div className="edit-session-head">
        <span>UNCOMMITTED · 手动编辑</span>
        <em>{summary.baseVersionId}</em>
      </div>
      <div className="edit-session-list">
        {summary.items.map((item, index) => (
          <span className="edit-op-row" key={`${index}-${item}`}>
            <b className={`edit-op-sign edit-op-sign--${editOpTone(item)}`}>
              {editOpSign(item)}
            </b>
            {item}
          </span>
        ))}
      </div>
      <div className="edit-session-actions">
        <button className="btn btn--primary btn--sm" disabled={busy} onClick={() => void onCommit()}>
          <Icon n="check" s={13} />
          提交编辑
        </button>
        <button className="btn btn--ghost btn--sm" disabled={busy} onClick={onDiscard}>
          放弃
        </button>
      </div>
    </div>
  );
}

function EditSessionIdleCard() {
  return (
    <div className="edit-session-card edit-session-card--idle">
      <div className="edit-session-head">
        <span>UNCOMMITTED · 等待编辑</span>
        <em>idle</em>
      </div>
      <div className="edit-session-empty">
        在画布移动节点、连线、改参数后，这里会列出待提交操作。
      </div>
      <div className="edit-session-actions">
        <button className="btn btn--primary btn--sm" disabled>
          <Icon n="check" s={13} />
          提交编辑
        </button>
        <button className="btn btn--ghost btn--sm" disabled>
          放弃
        </button>
      </div>
    </div>
  );
}

function RunErrorCard({
  run,
  busy,
  onRequestFix,
}: {
  run: WorkbenchState['run'];
  busy: boolean;
  onRequestFix: (text: string) => Promise<void>;
}) {
  const [rawOpen, setRawOpen] = useState(false);
  if (!run || run.status !== 'failed') return null;
  const failedSteps = run.steps.filter((step) => step.state === 'failed');
  const error = run.error ?? failedSteps.find((step) => step.error)?.error ?? null;
  const raw = error?.raw?.trim();
  const failedNodeText = failedSteps.map((step) => step.nodeId).join(', ') || 'unknown';

  return (
    <div className="run-error-card">
      <div className="run-error-head">
        <span className="run-error-icon">
          <Icon n="alert" s={13} />
        </span>
        <div>
          <div className="run-error-title">运行失败</div>
          <div className="run-error-meta">
            {run.label}
            {failedSteps.length > 0 ? ` · failed: ${failedSteps.map((step) => step.nodeId).join(', ')}` : ''}
          </div>
        </div>
      </div>
      <div className="run-error-summary">{error?.summary ?? '最近一次运行失败，暂无结构化错误摘要。'}</div>
      <button
        className="run-error-fix"
        disabled={busy}
        onClick={() =>
          void onRequestFix(
            `读取当前失败节点 ${failedNodeText} 和运行错误，生成最小修复并自动应用到工作流。`,
          ).catch(() => undefined)
        }
      >
        Auto fix workflow
      </button>
      {raw && (
        <>
          <button className="run-error-toggle" onClick={() => setRawOpen((open) => !open)}>
            {rawOpen ? '收起 raw error' : '查看 raw error'}
          </button>
          {rawOpen && <pre className="run-error-raw">{raw}</pre>}
        </>
      )}
    </div>
  );
}

function editOpSign(item: string): '+' | '~' | '-' {
  if (/^(Add|Connect)/.test(item)) return '+';
  if (/^(Remove|Disconnect)/.test(item)) return '-';
  return '~';
}

function editOpTone(item: string): 'add' | 'upd' | 'del' {
  const sign = editOpSign(item);
  if (sign === '+') return 'add';
  if (sign === '-') return 'del';
  return 'upd';
}

export function shouldSubmitComposerKey(event: ComposerKeyEvent, isComposing = false): boolean {
  if (event.key !== 'Enter' || event.shiftKey) return false;
  return !isImeComposing(event, isComposing);
}

function isImeComposing(event: ComposerKeyEvent, isComposing: boolean): boolean {
  return isComposing
    || event.nativeEvent.isComposing === true
    || event.nativeEvent.keyCode === 229
    || event.nativeEvent.which === 229;
}

function ProposalMessage({
  proposal,
  busy,
  onApply,
  onDismiss,
}: {
  proposal: PendingProposal;
  busy: boolean;
  onApply: () => Promise<void>;
  onDismiss: () => Promise<void>;
}) {
  return (
    <div className="msg">
      <div className="msg-avatar msg-avatar--agent">
        <Icon n="spark" s={13} fill />
      </div>
      <div className="msg-body">
        <div className="msg-name">Agent</div>
        <div className="msg-text dim proposal-intro">
          检测到旧的待处理图变更，画布正在预览这次变更。
        </div>
        <ProposalCard
          busy={busy}
          onApply={onApply}
          onDismiss={onDismiss}
          proposal={proposal}
        />
      </div>
    </div>
  );
}

function ProposalCard({
  proposal,
  busy,
  onApply,
  onDismiss,
}: {
  proposal: PendingProposal;
  busy: boolean;
  onApply: () => Promise<void>;
  onDismiss: () => Promise<void>;
}) {
  const [diffOpen, setDiffOpen] = useState(false);
  const diffRows = proposal.diffSummary.length > 0
    ? proposal.diffSummary
    : ['~ Graph preview ready'];

  return (
    <div className="prop">
      <div className="prop-head">
        <span className="prop-badge">
          <Icon n="spark" s={11} fill />
          图变更提议
        </span>
        <span className="prop-title">{proposal.title}</span>
      </div>
      <div className="prop-summary">{proposal.summary}</div>
      <div className="prop-changes">
        {diffRows.map((item) => (
          <div className="change-row" key={item}>
            <span className={`change-tag ${changeKind(item)}`}>{changeSymbol(item)}</span>
            <span className="lbl">{item}</span>
          </div>
        ))}
        {diffOpen && (
          <pre className="prop-diff">
            {diffRows.map((item) => item.trim()).join('\n')}
          </pre>
        )}
      </div>
      <div className="prop-actions">
        <button className="btn btn--primary btn--sm" disabled={busy} onClick={() => void onApply()}>
          <Icon n="check" s={13} />
          应用到画布
        </button>
        <button
          className="btn btn--ghost btn--sm"
          disabled={busy}
          onClick={() => setDiffOpen((open) => !open)}
        >
          {diffOpen ? '收起 Diff' : '查看 Diff'}
        </button>
        <span className="spacer" />
        <button className="btn btn--quiet btn--sm" disabled={busy} onClick={() => void onDismiss()}>
          忽略
        </button>
      </div>
    </div>
  );
}

function changeKind(item: string): string {
  if (item.startsWith('+')) return 'add';
  if (item.startsWith('-')) return 'del';
  return 'upd';
}

function changeSymbol(item: string): string {
  if (item.startsWith('+')) return '+';
  if (item.startsWith('-')) return '-';
  return '~';
}

function MessageRow({ message }: { message: ChatMessage }) {
  if (message.role === 'system') {
    return <div className="sys-msg">{message.text}</div>;
  }

  return (
    <div className="msg msg--user">
      <div className="msg-avatar msg-avatar--user">
        你
      </div>
      <div className="msg-body">
        <div className="msg-name">你</div>
        <div className="msg-text">
          <span className="bubble">{message.text}</span>
        </div>
        <time>{formatTime(message.time)}</time>
      </div>
    </div>
  );
}

function MessageTimeline({ messages }: { messages: ChatMessage[] }) {
  const entries = chatEntries(messages);
  if (entries.length === 0) return null;

  return (
    <>
      {entries.map((entry) =>
        entry.type === 'message' ? (
          <MessageRow key={entry.message.id} message={entry.message} />
        ) : (
          <AssistantTurn
            key={entry.id}
            logs={entry.logs}
            message={entry.message}
          />
        ),
      )}
    </>
  );
}

function AssistantTurn({
  message,
  logs,
}: {
  message?: ChatMessage;
  logs: ChatMessage[];
}) {
  const isStatus = message ? isAgentStatusMessage(message) : false;
  return (
    <div className="msg msg--assistant">
      <div className="msg-avatar msg-avatar--agent">
        <Icon n="spark" s={13} fill />
      </div>
      <div className="msg-body">
        <div className="msg-name">Agent</div>
        {message ? (
          <div
            className={
              message.kind === 'clarify' ? 'assistant-bubble clarify-bubble' : 'assistant-bubble'
            }
          >
            {isStatus ? (
              <div className="status-line">
                <span className="spin p-rotating" />
                {message.text}
              </div>
            ) : message.kind === 'clarify' ? (
              <div className="msg-text clarify-text" data-testid="clarify-message">
                {message.text}
              </div>
            ) : (
              <div className="msg-text">{message.text}</div>
            )}
          </div>
        ) : (
          <div className="assistant-bubble">
            <div className="status-line">
              <span className="spin p-rotating" />
              Agent 正在执行工具调用
            </div>
          </div>
        )}
        {logs.length > 0 && <ToolCallGroup messages={logs} />}
        <time>{formatTime(message?.time ?? logs.at(-1)?.time ?? '')}</time>
      </div>
    </div>
  );
}

function ToolCallGroup({ messages }: { messages: ChatMessage[] }) {
  const [open, setOpen] = useState(false);
  const compactMessages = compactToolMessages(messages);
  const title = toolGroupTitle(compactMessages);
  return (
    <div className="agent-log-group">
      <button className="agent-log-toggle" onClick={() => setOpen((value) => !value)}>
        <span className="agent-log-toggle-title">{title}</span>
        <span className="agent-log-toggle-meta">
          {compactMessages.length} events · {formatTime(compactMessages.at(-1)?.time ?? '')}
        </span>
        <span className="agent-log-toggle-caret">{open ? '收起' : '展开'}</span>
      </button>
      {open && (
        <div className="agent-log-list">
          {compactMessages.map((message) => (
            <AgentLogRow key={message.id} message={message} />
          ))}
        </div>
      )}
    </div>
  );
}

function AgentLogRow({ message }: { message: ChatMessage }) {
  const [rawOpen, setRawOpen] = useState(false);
  return (
    <div className="agent-log-row">
      <div className="agent-log-row-head">
        <span className="agent-log-label">{cleanLogLabel(message)}</span>
        <time>{formatTime(message.time)}</time>
      </div>
      <pre className="agent-log-body">{compactLogText(message.text)}</pre>
      {message.raw && (
        <>
          <button className="agent-log-raw-btn" onClick={() => setRawOpen((open) => !open)}>
            {rawOpen ? '收起 raw' : '查看 raw'}
          </button>
          {rawOpen && <pre className="agent-log-raw">{message.raw}</pre>}
        </>
      )}
    </div>
  );
}

function chatEntries(messages: ChatMessage[]): ChatEntry[] {
  const entries: ChatEntry[] = [];
  let pendingLogs: ChatMessage[] = [];
  for (const message of messages) {
    if (isHiddenAgentLog(message)) {
      continue;
    }
    if (message.role === 'system' || message.role === 'user') {
      flushPendingLogs(entries, pendingLogs);
      pendingLogs = [];
      entries.push({ type: 'message', message });
      continue;
    }
    if (isAgentLog(message)) {
      pendingLogs.push(message);
      continue;
    }
    if (message.role === 'agent') {
      const last = lastAssistantTurn(entries);
      if (last?.message && isAgentStatusMessage(last.message)) {
        entries.pop();
      }
      entries.push({
        type: 'assistantTurn',
        id: `assistant-${message.id}`,
        message,
        logs: pendingLogs,
      });
      pendingLogs = [];
      continue;
    }
  }
  flushPendingLogs(entries, pendingLogs);
  return entries;
}

function lastAssistantTurn(entries: ChatEntry[]) {
  const last = entries.at(-1);
  return last?.type === 'assistantTurn' ? last : null;
}

function flushPendingLogs(entries: ChatEntry[], logs: ChatMessage[]) {
  if (logs.length === 0) return;
  const turn = lastAssistantTurn(entries);
  if (turn) {
    turn.logs.push(...logs);
  } else {
    entries.push({
      type: 'assistantTurn',
      id: `assistant-${logs[0].id}`,
      logs: [...logs],
    });
  }
}

function isAgentLog(message: ChatMessage): boolean {
  return message.kind?.startsWith('agent_log:') ?? false;
}

function isAgentStatusMessage(message: ChatMessage): boolean {
  return message.id.startsWith('agent-status-') || message.kind === 'agent_status';
}

function isHiddenAgentLog(message: ChatMessage): boolean {
  const kind = message.kind?.replace(/^agent_log:/, '');
  return (
    kind === 'thread_started' ||
    kind === 'turn_started' ||
    kind === 'turn_completed' ||
    kind === 'turn_sent' ||
    kind === 'response_started' ||
    kind === 'response_completed' ||
    kind === 'assistant_message' ||
    kind === 'file_change' ||
    isInternalWarning(message)
  );
}

function isInternalWarning(message: ChatMessage): boolean {
  return (
    message.kind === 'agent_log:error' &&
    message.text.includes('Skill descriptions were shortened')
  );
}

function compactToolMessages(messages: ChatMessage[]): ChatMessage[] {
  const compacted: ChatMessage[] = [];
  const commandIndexes = new Map<string, number>();
  for (const message of messages) {
    if (message.kind?.includes('command_execution')) {
      const command = commandText(message);
      const existingIndex = commandIndexes.get(command);
      if (existingIndex !== undefined) {
        if (message.text.length >= compacted[existingIndex].text.length) {
          compacted[existingIndex] = message;
        }
        continue;
      }
      commandIndexes.set(command, compacted.length);
    }
    compacted.push(message);
  }
  return compacted;
}

function commandText(message: ChatMessage): string {
  return message.text.split('\n', 1)[0].trim();
}

function toolGroupTitle(messages: ChatMessage[]): string {
  if (messages.some((message) => message.kind === 'agent_log:canvas_ops')) {
    return 'Canvas ops evidence';
  }
  const commandCount = messages.filter((message) =>
    message.kind?.includes('command_execution'),
  ).length;
  const errorCount = messages.filter((message) => message.kind?.includes('error')).length;
  if (errorCount > 0) return `工具调用 · ${commandCount} 次 · ${errorCount} 个事件`;
  if (commandCount > 0) return `工具调用 · ${commandCount} 次`;
  return '工具调用';
}

function cleanLogLabel(message: ChatMessage): string {
  if (message.kind === 'agent_log:canvas_ops') {
    return 'canvas ops';
  }
  return (message.label ?? message.kind ?? 'runtime')
    .replace(/^agent_log:/, '')
    .replaceAll('_', ' ');
}

function compactLogText(text: string): string {
  const trimmed = text.trim();
  if (trimmed.length <= 900) return trimmed;
  return `${trimmed.slice(0, 900)}\n...`;
}

export function formatTime(value: string): string {
  if (!value.includes('T')) return value;
  return new Date(value).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
}
