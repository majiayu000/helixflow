import { useEffect, useMemo, useState } from 'react';
import type { GraphNodeState, NodeDefinition, WorkflowGraph } from '../types';
import { categorySwatch } from './graph-canvas-rendering';

type WorkflowNode = WorkflowGraph['nodes'][string];
type ParamSpec = NodeDefinition['params_schema']['properties'][string];
type InspectorErrorMap = Record<string, string | undefined>;

export type InspectorControlKind = 'text' | 'number' | 'select' | 'checkbox' | 'readonly';

type GraphInspectorProps = {
  catalogError?: string | null;
  definition?: NodeDefinition;
  node: GraphNodeState;
  workflowNode?: WorkflowNode;
  onClose: () => void;
  onRequestProposal?: (nodeId: string) => Promise<void>;
  onSetParam?: (nodeId: string, key: string, value: unknown) => Promise<void>;
};

export function GraphInspector({
  catalogError,
  definition,
  node,
  workflowNode,
  onClose,
  onRequestProposal,
  onSetParam,
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

  return (
    <div className="inspector p-inspector" onClick={(event) => event.stopPropagation()}>
      <div className="inspector-head">
        <div className="kicker">选中节点 · {node.id}</div>
        <div className="title">
          <span style={{ background: categorySwatch(node.category) }} />
          {node.title}
        </div>
        <button className="p-close" onClick={onClose}>
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
        {onRequestProposal && (
          <button className="inspector-agent-request" onClick={() => void onRequestProposal(node.id)}>
            Ask Agent for node proposal
          </button>
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

export function GraphSelectionInspector({
  nodes,
  onClose,
}: {
  nodes: GraphNodeState[];
  onClose: () => void;
}) {
  const categories = [...new Set(nodes.map((node) => node.category))].sort();
  return (
    <div className="inspector p-inspector" onClick={(event) => event.stopPropagation()}>
      <div className="inspector-head">
        <div className="kicker">多选 · {nodes.length} 个节点</div>
        <div className="title">
          <span />
          Selection summary
        </div>
        <button className="p-close" onClick={onClose}>
          x
        </button>
      </div>
      <div className="inspector-body">
        <div className="field">
          <span className="field-label">categories</span>
          <span className="field-input">{categories.join(', ')}</span>
        </div>
        <div className="selection-summary-list">
          {nodes.map((node) => (
            <div className="selection-summary-row" key={node.id}>
              <span style={{ background: categorySwatch(node.category) }} />
              <div>
                <strong>{node.title}</strong>
                <small>{node.id}</small>
              </div>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
