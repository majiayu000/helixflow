import { useMemo, useState } from 'react';
import { isLocalCanvasActor, LOCAL_CANVAS_ACTOR } from '../canvas-presence';
import { useWorkbenchStore } from '../store';
import type {
  CanvasComment,
  CanvasCommentOpInput,
  CanvasCommentTarget,
  CanvasPresence,
  GraphNodeState,
  WorkbenchState,
} from '../types';
import { graphNodeHeight, graphNodeWidth, type ViewportSize, type ViewState } from './graph-canvas-navigation';

type PresenceByActor = Record<string, CanvasPresence>;

type CanvasCollaborationWorldProps = {
  comments: CanvasComment[];
  hiddenActorId?: string;
  nodes: GraphNodeState[];
  presenceByActor: PresenceByActor;
};

type ConnectedCanvasCollaborationWorldProps = Omit<
  CanvasCollaborationWorldProps,
  'hiddenActorId' | 'presenceByActor'
>;

export function ConnectedCanvasCollaborationWorld(
  props: ConnectedCanvasCollaborationWorldProps,
) {
  const presenceByActor = useWorkbenchStore((store) => store.presenceByActor);
  return (
    <CanvasCollaborationWorld
      {...props}
      hiddenActorId={LOCAL_CANVAS_ACTOR.actorId}
      presenceByActor={presenceByActor}
    />
  );
}

export function CanvasCollaborationWorld({
  comments,
  hiddenActorId,
  nodes,
  presenceByActor,
}: CanvasCollaborationWorldProps) {
  const nodeById = useMemo(() => new Map(nodes.map((node) => [node.id, node] as const)), [nodes]);
  return (
    <>
      {Object.values(presenceByActor)
        .filter((presence) =>
          presence.actor.actorId !== hiddenActorId &&
          !isLocalCanvasActor(presence.actor.actorId))
        .map((presence) => (
          <div className="collab-presence" key={presence.actor.actorId}>
            {presence.selection?.nodeIds.map((nodeId) => {
              const node = nodeById.get(nodeId);
              if (!node) return null;
              return (
                <span
                  className="collab-selection"
                  key={nodeId}
                  style={{
                    height: graphNodeHeight(node) + 10,
                    left: node.position.x - 5,
                    top: node.position.y - 5,
                    width: graphNodeWidth(node) + 10,
                  }}
                />
              );
            })}
            {presence.cursor && (
              <span
                className="collab-cursor"
                style={{
                  transform: `translate3d(${presence.cursor.x + 8}px, ${presence.cursor.y + 8}px, 0)`,
                }}
              >
                {presence.actor.displayName}
              </span>
            )}
          </div>
        ))}
      {comments.map((comment) => {
        const point = commentPoint(comment, nodeById);
        if (!point) return null;
        return (
          <span
            className={`comment-marker comment-marker--${comment.status}`}
            key={comment.id}
            style={{ left: point.x, top: point.y }}
            title={comment.body}
          >
            {comment.status === 'resolved' ? '✓' : '•'}
          </span>
        );
      })}
    </>
  );
}

type CanvasCommentsPanelProps = {
  comments: CanvasComment[];
  edges: WorkbenchState['graph']['edges'];
  nodes: GraphNodeState[];
  onCommentOp?: (input: CanvasCommentOpInput) => Promise<void>;
  selectedNodeId?: string;
  view: ViewState;
  viewportSize: ViewportSize;
};

export function CanvasCommentsPanel({
  comments,
  edges,
  nodes,
  onCommentOp,
  selectedNodeId,
  view,
  viewportSize,
}: CanvasCommentsPanelProps) {
  const [draft, setDraft] = useState('');
  const [open, setOpen] = useState(comments.length > 0);
  const targetOptions = useMemo(
    () => commentTargetOptions(nodes, edges, selectedNodeId, view, viewportSize),
    [edges, nodes, selectedNodeId, view, viewportSize],
  );
  const [targetKey, setTargetKey] = useState(targetOptions[0]?.key ?? '');
  const selectedTarget =
    targetOptions.find((target) => target.key === targetKey) ?? targetOptions[0];
  const canSubmit = Boolean(onCommentOp && selectedTarget && draft.trim());

  const submit = async () => {
    if (!onCommentOp || !selectedTarget) return;
    try {
      await onCommentOp({
        op: {
          op: 'comment_add',
          target: selectedTarget.target,
          body: draft,
        },
      });
      setDraft('');
    } catch {
      // The store owns visible errors; keep the draft for retry.
    }
  };

  if (!open) {
    return (
      <button
        aria-expanded="false"
        className="comment-toggle"
        onClick={() => setOpen(true)}
        onPointerDown={(event) => event.stopPropagation()}
        type="button"
      >
        Comments <strong>{comments.length}</strong>
      </button>
    );
  }

  return (
    <aside className="comment-panel" onPointerDown={(event) => event.stopPropagation()}>
      <div className="comment-panel-head">
        <span>Comments</span>
        <button onClick={() => setOpen(false)} type="button">
          {comments.length}
        </button>
      </div>
      <div className="comment-compose">
        <select
          disabled={!onCommentOp}
          onChange={(event) => setTargetKey(event.currentTarget.value)}
          value={selectedTarget?.key ?? ''}
        >
          {targetOptions.map((target) => (
            <option key={target.key} value={target.key}>
              {target.label}
            </option>
          ))}
        </select>
        <textarea
          disabled={!onCommentOp}
          onChange={(event) => setDraft(event.currentTarget.value)}
          placeholder="Add comment"
          value={draft}
        />
        <button disabled={!canSubmit} onClick={() => void submit()}>
          Add
        </button>
      </div>
      <div className="comment-list">
        {comments.map((comment) => (
          <div className={`comment-row comment-row--${comment.status}`} key={comment.id}>
            <div>
              <strong>{comment.author.displayName}</strong>
              <span>{commentTargetLabel(comment.target)}</span>
            </div>
            <p>{comment.body}</p>
            <div className="comment-actions">
              <button
                disabled={!onCommentOp}
                onClick={() => void patchCommentBody(comment, onCommentOp).catch(() => undefined)}
              >
                Edit
              </button>
              <button
                disabled={!onCommentOp}
                onClick={() =>
                  void onCommentOp?.({
                    op: {
                      op: 'comment_patch',
                      id: comment.id,
                      status: comment.status === 'resolved' ? 'open' : 'resolved',
                    },
                  }).catch(() => undefined)
                }
              >
                {comment.status === 'resolved' ? 'Reopen' : 'Resolve'}
              </button>
              <button
                disabled={!onCommentOp}
                onClick={() => void onCommentOp?.({
                  op: { op: 'comment_delete', id: comment.id },
                }).catch(() => undefined)}
              >
                Delete
              </button>
            </div>
          </div>
        ))}
      </div>
    </aside>
  );
}

type CommentTargetOption = {
  key: string;
  label: string;
  target: CanvasCommentTarget;
};

function commentTargetOptions(
  nodes: GraphNodeState[],
  edges: WorkbenchState['graph']['edges'],
  selectedNodeId: string | undefined,
  view: ViewState,
  viewportSize: ViewportSize,
): CommentTargetOption[] {
  const selectedNode = selectedNodeId ? nodes.find((node) => node.id === selectedNodeId) : undefined;
  const position = {
    x: (viewportSize.width / 2 - view.x) / view.z,
    y: (viewportSize.height / 2 - view.y) / view.z,
  };
  return [
    ...(selectedNode
      ? [
          {
            key: `node:${selectedNode.id}`,
            label: `Node · ${selectedNode.title}`,
            target: { kind: 'node' as const, nodeId: selectedNode.id },
          },
        ]
      : []),
    ...edges.slice(0, 8).map((edge) => ({
      key: `edge:${edge.id}`,
      label: `Edge · ${edge.from.nodeId} -> ${edge.to.nodeId}`,
      target: { kind: 'edge' as const, edgeId: edge.id },
    })),
    {
      key: 'position:center',
      label: 'Canvas position',
      target: { kind: 'position', x: position.x, y: position.y },
    },
  ];
}

function commentPoint(
  comment: CanvasComment,
  nodeById: Map<string, GraphNodeState>,
): { x: number; y: number } | null {
  if (comment.target.kind === 'position') {
    return { x: comment.target.x, y: comment.target.y };
  }
  if (comment.target.kind === 'node') {
    const node = nodeById.get(comment.target.nodeId);
    return node
      ? { x: node.position.x + graphNodeWidth(node) - 8, y: node.position.y - 8 }
      : null;
  }
  return { x: 16, y: 16 };
}

function commentTargetLabel(target: CanvasCommentTarget): string {
  if (target.kind === 'node') return `node:${target.nodeId}`;
  if (target.kind === 'edge') return `edge:${target.edgeId}`;
  return `position:${Math.round(target.x)},${Math.round(target.y)}`;
}

async function patchCommentBody(
  comment: CanvasComment,
  onCommentOp: CanvasCommentsPanelProps['onCommentOp'],
) {
  if (!onCommentOp || typeof window === 'undefined') return;
  const body = window.prompt('Edit comment', comment.body);
  if (!body || body === comment.body) return;
  await onCommentOp({ op: { op: 'comment_patch', id: comment.id, body } });
}
