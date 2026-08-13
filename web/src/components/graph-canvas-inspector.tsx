import { useEffect, useMemo, useState, type CSSProperties } from 'react';
import { Icon } from '../icons';
import type {
  GraphNodeState,
  ImplementationResolution,
  NodeDefinition,
  WorkflowGraph,
} from '../types';
import { graphNodeHeight, graphNodeWidth, type ViewState, type ViewportSize } from './graph-canvas-navigation';
import { categorySwatch } from './graph-canvas-rendering';

type WorkflowNode = WorkflowGraph['nodes'][string];
type ParamSpec = NodeDefinition['params_schema']['properties'][string];
type InspectorErrorMap = Record<string, string | undefined>;

export type InspectorControlKind = 'text' | 'number' | 'select' | 'checkbox' | 'readonly';

/// Shows what the node will actually execute (GH130 T5): capability, the
/// resolved model/binding via the catalog, or the stable reason it cannot
/// run. A clarify-grade failure is presented as exactly that — never as a
/// resolved implementation.
function ImplementationSection({
  capability,
  resolution,
}: {
  capability: string;
  resolution: ImplementationResolution | null;
}) {
  return (
    <div className="inspector-implementation" data-testid="implementation-section">
      <div className="field">
        <span className="field-label">capability</span>
        <span className="field-input">{capability}</span>
      </div>
      {!resolution && (
        <div className="field">
          <span className="field-label">implementation</span>
          <span className="field-input">解析中…</span>
        </div>
      )}
      {resolution?.status === 'resolved' && (
        <>
          <div className="field">
            <span className="field-label">model</span>
            <span className="field-input">{resolution.resolved.resolvedModelId}</span>
          </div>
          <div className="field">
            <span className="field-label">binding</span>
            <span className="field-input">
              {resolution.resolved.bindingId} ({resolution.resolved.bindingRevision})
            </span>
          </div>
          {'apiConnector' in resolution.resolved.target && (
            <div className="field">
              <span className="field-label">connector</span>
              <span className="field-input">
                {resolution.resolved.target.apiConnector.connectorId}
              </span>
            </div>
          )}
        </>
      )}
      {resolution?.status === 'unresolvable' && (
        <div className="field">
          <span className="field-label">不可运行</span>
          <span className="field-input inspector-unresolvable">
            [{resolution.code}] {resolution.message}
          </span>
        </div>
      )}
    </div>
  );
}

type GraphInspectorProps = {
  catalogError?: string | null;
  definition?: NodeDefinition;
  node: GraphNodeState;
  resolution?: ImplementationResolution | null;
  workflowNode?: WorkflowNode;
  onClose: () => void;
  onRequestProposal?: (nodeId: string) => Promise<void>;
  onSetParam?: (nodeId: string, key: string, value: unknown) => Promise<void>;
  view?: ViewState;
  viewportSize?: ViewportSize;
};

export function GraphInspector({
  catalogError,
  definition,
  node,
  workflowNode,
  onClose,
  onRequestProposal,
  onSetParam,
  view,
  viewportSize,
  resolution,
}: GraphInspectorProps) {
  const paramObject = useMemo(() => paramsObject(workflowNode), [workflowNode]);
  const fields = useMemo(
    () => inspectorFields(paramObject, definition),
    [definition, paramObject],
  );
  const [drafts, setDrafts] = useState<Record<string, string>>(() =>
    initialDrafts(fields),
  );
  const [errors, setErrors] = useState<InspectorErrorMap>({});
  const [savingKey, setSavingKey] = useState<string | null>(null);
  const readonlyReason = !workflowNode
    ? '当前 workflow graph 不可用'
    : !definition
      ? 'schema 缺失，只读'
      : !onSetParam
        ? '当前状态不可编辑'
        : null;

  useEffect(() => {
    setDrafts(initialDrafts(fields));
    setErrors({});
    setSavingKey(null);
  }, [fields, node.id]);

  const updateDraft = (key: string, value: string) => {
    setDrafts((current) => ({ ...current, [key]: value }));
    setErrors((current) => ({ ...current, [key]: undefined }));
  };

  const saveParam = async (field: InspectorField) => {
    if (!onSetParam || savingKey) return;
    const draft = drafts[field.key] ?? '';
    const result = validateParamDraft(field.key, draft, field.spec, field.required);
    if (!result.ok) {
      setErrors((current) => ({ ...current, [field.key]: result.error }));
      return;
    }

    setSavingKey(field.key);
    try {
      await onSetParam(node.id, field.key, result.value);
      setErrors((current) => ({ ...current, [field.key]: undefined }));
    } catch (error) {
      const message = error instanceof Error ? error.message : '参数保存失败';
      setErrors((current) => ({ ...current, [field.key]: message }));
    } finally {
      setSavingKey(null);
    }
  };

  const style = nodeInspectorStyle(
    node,
    view ?? { x: 0, y: 0, z: 1 },
    viewportSize ?? { width: 900, height: 640 },
  );
  return (
    <div
      className="inspector p-inspector inspector-floating inspector-compact"
      onClick={(event) => event.stopPropagation()}
      style={style}
    >
      <span className="selection-summary-accessible">
        选中节点 · {node.id} · {node.title}
      </span>
      <div className="inspector-node-actions">
        <details className="inspector-edit-details">
          <summary
            className="inspector-tool"
            onClick={(event) => event.stopPropagation()}
            onPointerDown={(event) => event.stopPropagation()}
            title="编辑参数"
          >
            <Icon n="sliders" s={14} />
            <span>参数</span>
          </summary>
          <div className="inspector-details-panel">
            <div className="inspector-head">
              <div className="kicker">选中节点 · {node.id}</div>
              <div className="title">
                <span style={{ background: categorySwatch(node.category) }} />
                {node.title}
              </div>
              <button className="p-close" onClick={onClose} type="button">
                x
              </button>
            </div>
            <div className="inspector-body">
              {catalogError && <div className="inspector-error">{catalogError}</div>}
              <div className="field">
                <span className="field-label">node type</span>
                <span className="field-input">{node.nodeType}</span>
              </div>
              <div className="field">
                <span className="field-label">provider</span>
                <span className="field-input">{node.provider ?? 'local/builtin'}</span>
              </div>
              {definition?.capability && (
                <ImplementationSection
                  capability={definition.capability}
                  resolution={resolution ?? null}
                />
              )}
              {fields.length === 0 && (
                <div className="field">
                  <span className="field-label">params</span>
                  <span className="field-input field-area">{node.summary}</span>
                </div>
              )}
              {fields.map((field) => (
                <InspectorParamField
                  disabledReason={readonlyReason}
                  draft={drafts[field.key] ?? ''}
                  error={errors[field.key]}
                  field={field}
                  key={field.key}
                  onChange={(value) => updateDraft(field.key, value)}
                  onRandomSeed={() => updateDraft(field.key, String(randomSeed()))}
                  onSave={() => void saveParam(field)}
                  saving={savingKey === field.key}
                />
              ))}
            </div>
          </div>
        </details>
        <button
          className="inspector-tool inspector-tool-primary"
          disabled={!onRequestProposal}
          onClick={() => void onRequestProposal?.(node.id).catch(() => undefined)}
          type="button"
        >
          <Icon n="play" s={13} fill />
          生成
        </button>
        <button
          className="inspector-tool"
          disabled={!onRequestProposal}
          onClick={() => void onRequestProposal?.(node.id).catch(() => undefined)}
          type="button"
        >
          <Icon n="spark" s={13} />
          对话
          <span className="selection-summary-accessible">Ask Agent for node proposal</span>
        </button>
        <button className="inspector-tool inspector-tool-close" onClick={onClose} title="取消选择" type="button">
          x
        </button>
      </div>
    </div>
  );
}

type InspectorField = {
  key: string;
  required: boolean;
  spec?: ParamSpec;
  value: unknown;
};

function InspectorParamField({
  disabledReason,
  draft,
  error,
  field,
  onChange,
  onRandomSeed,
  onSave,
  saving,
}: {
  disabledReason: string | null;
  draft: string;
  error?: string;
  field: InspectorField;
  onChange: (value: string) => void;
  onRandomSeed: () => void;
  onSave: () => void;
  saving: boolean;
}) {
  const kind = controlKindForParam(field.spec);
  const disabled = Boolean(disabledReason) || kind === 'readonly' || saving;
  const canRandomSeed = field.key.toLowerCase().includes('seed') && field.spec?.type === 'integer';

  return (
    <form
      className="field inspector-field"
      onSubmit={(event) => {
        event.preventDefault();
        if (!disabled) onSave();
      }}
    >
      <span className="field-label">
        {field.key}
        {field.required && <em>required</em>}
      </span>
      <div className="inspector-control-row">
        {kind === 'select' ? (
          <select
            className="field-input inspector-control"
            disabled={disabled}
            onChange={(event) => onChange(event.currentTarget.value)}
            value={draft}
          >
            <option value="">选择...</option>
            {(field.spec?.enum_values ?? []).map((value) => (
              <option key={enumOptionValue(value)} value={enumOptionValue(value)}>
                {displayParamValue(value)}
              </option>
            ))}
          </select>
        ) : kind === 'checkbox' ? (
          <input
            checked={draft === 'true'}
            className="inspector-checkbox"
            disabled={disabled}
            onChange={(event) => onChange(event.currentTarget.checked ? 'true' : 'false')}
            type="checkbox"
          />
        ) : kind === 'readonly' ? (
          <span className="field-input field-area">{displayParamValue(field.value)}</span>
        ) : kind === 'text' && multilineParam(field.key, draft) ? (
          <textarea
            className="field-input inspector-control inspector-textarea"
            disabled={disabled}
            onChange={(event) => onChange(event.currentTarget.value)}
            value={draft}
          />
        ) : (
          <input
            className="field-input inspector-control"
            disabled={disabled}
            inputMode={kind === 'number' ? 'decimal' : undefined}
            onChange={(event) => onChange(event.currentTarget.value)}
            type={kind === 'number' ? 'number' : 'text'}
            value={draft}
          />
        )}
        {canRandomSeed && (
          <button
            className="inspector-icon-btn"
            disabled={disabled}
            onClick={onRandomSeed}
            title="随机 seed"
            type="button"
          >
            #
          </button>
        )}
        {kind !== 'readonly' && (
          <button className="inspector-save" disabled={disabled} type="submit">
            {saving ? '保存中' : '保存'}
          </button>
        )}
      </div>
      {(error || disabledReason || kind === 'readonly') && (
        <small className={error ? 'field-error' : 'field-hint'}>
          {error ?? disabledReason ?? 'schema 缺失，只读'}
        </small>
      )}
    </form>
  );
}

export function controlKindForParam(spec?: ParamSpec): InspectorControlKind {
  if (!spec) return 'readonly';
  if (spec.enum_values.length > 0) return 'select';
  if (spec.type === 'integer' || spec.type === 'number') return 'number';
  if (spec.type === 'boolean') return 'checkbox';
  if (spec.type === 'string') return 'text';
  return 'readonly';
}

export function validateParamDraft(
  key: string,
  draft: string,
  spec: ParamSpec | undefined,
  required: boolean,
): { ok: true; value: unknown } | { ok: false; error: string } {
  if (!spec) return { ok: false, error: '缺少 schema，无法编辑' };
  const trimmed = draft.trim();
  if (required && trimmed.length === 0) {
    return { ok: false, error: `${key} 为必填` };
  }
  if (!required && trimmed.length === 0 && spec.type !== 'boolean') {
    return { ok: false, error: `${key} 请输入值` };
  }
  if (spec.enum_values.length > 0) {
    const match = spec.enum_values.find((value) => enumOptionValue(value) === draft);
    return match === undefined
      ? { ok: false, error: `${key} 不在允许选项内` }
      : { ok: true, value: match };
  }
  if (spec.type === 'integer' || spec.type === 'number') {
    const value = Number(trimmed);
    if (!Number.isFinite(value)) return { ok: false, error: `${key} 必须是数字` };
    if (spec.type === 'integer' && !Number.isInteger(value)) {
      return { ok: false, error: `${key} 必须是整数` };
    }
    if (spec.minimum !== null && value < spec.minimum) {
      return { ok: false, error: `${key} 不能小于 ${spec.minimum}` };
    }
    if (spec.maximum !== null && value > spec.maximum) {
      return { ok: false, error: `${key} 不能大于 ${spec.maximum}` };
    }
    return { ok: true, value };
  }
  if (spec.type === 'boolean') {
    return { ok: true, value: draft === 'true' };
  }
  return { ok: true, value: draft };
}

function inspectorFields(
  params: Record<string, unknown>,
  definition?: NodeDefinition,
): InspectorField[] {
  const schema = definition?.params_schema;
  const schemaKeys = Object.keys(schema?.properties ?? {});
  const keys = [...new Set([...schemaKeys, ...Object.keys(params)])];
  return keys.map((key) => ({
    key,
    required: Boolean(schema?.required.includes(key)),
    spec: schema?.properties[key],
    value: params[key],
  }));
}

function initialDrafts(fields: InspectorField[]): Record<string, string> {
  return Object.fromEntries(
    fields.map((field) => [
      field.key,
      initialDraftValue(field.value, field.spec),
    ]),
  );
}

function initialDraftValue(value: unknown, spec?: ParamSpec): string {
  if (spec?.enum_values.length) {
    const found = spec.enum_values.find((item) => item === value);
    return found === undefined ? '' : enumOptionValue(found);
  }
  if (typeof value === 'boolean') return value ? 'true' : 'false';
  if (value === null || value === undefined) return '';
  if (typeof value === 'string' || typeof value === 'number') return String(value);
  return JSON.stringify(value);
}

function paramsObject(workflowNode?: WorkflowNode): Record<string, unknown> {
  const params = workflowNode?.params;
  return params && typeof params === 'object' && !Array.isArray(params)
    ? (params as Record<string, unknown>)
    : {};
}

function enumOptionValue(value: unknown): string {
  return JSON.stringify(value);
}

function displayParamValue(value: unknown): string {
  if (value === undefined) return '';
  if (typeof value === 'string') return value;
  return JSON.stringify(value);
}

function randomSeed(): number {
  return Math.floor(Math.random() * 1_000_000);
}

function multilineParam(key: string, draft: string): boolean {
  const normalized = key.toLowerCase();
  return normalized.includes('prompt') || normalized.includes('brief') || draft.length > 56;
}

export function GraphSelectionInspector({
  nodes,
  onCopy,
  onDelete,
  onClose,
  view,
  viewportSize,
}: {
  nodes: GraphNodeState[];
  onCopy?: () => void;
  onDelete?: () => void;
  onClose: () => void;
  view?: ViewState;
  viewportSize?: ViewportSize;
}) {
  const categories = [...new Set(nodes.map((node) => node.category))].sort();
  const style = selectionToolbarStyle(
    nodes,
    view ?? { x: 0, y: 0, z: 1 },
    viewportSize ?? { width: 900, height: 640 },
  );
  return (
    <div className="selection-actionbar" onClick={(event) => event.stopPropagation()} style={style}>
      <span className="selection-summary-accessible">
        多选 · {nodes.length} 个节点 · Selection summary · {nodes.map((node) => node.title).join(', ')}
      </span>
      <span className="selection-count">{nodes.length} selected</span>
      <span className="selection-categories" title={categories.join(', ')}>
        {categories.join(' · ')}
      </span>
      <button onClick={onCopy} type="button">
        <Icon n="paperclip" s={13} />
        复制
      </button>
      <button onClick={onDelete} type="button">
        <Icon n="eraser" s={13} />
        删除
      </button>
      <button onClick={onClose} type="button">
        取消
      </button>
    </div>
  );
}

function selectionToolbarStyle(
  nodes: GraphNodeState[],
  view: ViewState,
  viewportSize: ViewportSize,
): CSSProperties {
  const bounds = nodes.reduce(
    (current, node) => {
      const left = node.position.x;
      const top = node.position.y;
      const right = left + graphNodeWidth(node);
      const bottom = top + graphNodeHeight(node);
      return {
        minX: Math.min(current.minX, left),
        minY: Math.min(current.minY, top),
        maxX: Math.max(current.maxX, right),
        maxY: Math.max(current.maxY, bottom),
      };
    },
    { minX: Infinity, minY: Infinity, maxX: -Infinity, maxY: -Infinity },
  );
  const x = (bounds.minX + (bounds.maxX - bounds.minX) / 2) * view.z + view.x;
  const y = bounds.maxY * view.z + view.y + 14;
  return {
    left: Math.min(Math.max(14, x), viewportSize.width - 14),
    top: Math.min(Math.max(108, y), viewportSize.height - 84),
    transform: 'translateX(-50%)',
  };
}

function nodeInspectorStyle(
  node: GraphNodeState,
  view: ViewState,
  viewportSize: ViewportSize,
): CSSProperties {
  const panelWidth = 316;
  const gap = 12;
  const nodeLeft = node.position.x * view.z + view.x;
  const nodeTop = node.position.y * view.z + view.y;
  const nodeRight = nodeLeft + graphNodeWidth(node) * view.z;
  const nodeCenter = nodeLeft + (graphNodeWidth(node) * view.z) / 2;
  const topSide = nodeTop - 46 - gap;
  const left = Math.min(nodeRight - panelWidth, nodeCenter - panelWidth / 2);
  const maxLeft = Math.max(14, viewportSize.width - panelWidth - 14);
  const maxTop = Math.max(92, viewportSize.height - 96);
  return {
    left: Math.min(Math.max(14, left), maxLeft),
    top: Math.min(Math.max(92, topSide), maxTop),
  };
}
