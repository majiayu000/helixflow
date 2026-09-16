import { useState, type FormEvent } from 'react';
import { Icon } from '../icons';
import {
  defaultImageCanvasToolRequest,
  imageCanvasToolLabel,
  type ImageCanvasToolKind,
  type ImageCanvasToolRequest,
} from '../image-canvas-tools';
import { useImageProcessingProfile } from '../use-image-processing-profile';

export function ImageCanvasToolsPanel({
  disabled,
  onApply,
  workspaceId,
}: {
  disabled?: boolean;
  onApply: (request: ImageCanvasToolRequest) => Promise<void>;
  workspaceId: string;
}) {
  const [kind, setKind] = useState<ImageCanvasToolKind>('outpaint');
  const [request, setRequest] = useState<ImageCanvasToolRequest>(() =>
    defaultImageCanvasToolRequest('outpaint'),
  );
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [quality, setQuality] = useState<'low' | 'medium' | 'high'>('low');
  const [sizeTier, setSizeTier] = useState<'1k' | '2k' | '4k'>('1k');
  const processor = useImageProcessingProfile(workspaceId, kind);

  const selectKind = (next: ImageCanvasToolKind) => {
    setKind(next);
    setRequest(defaultImageCanvasToolRequest(next));
    setError(null);
  };

  const submit = (event: FormEvent) => {
    event.preventDefault();
    setError(null);
    setBusy(true);
    const submitted = request.kind === 'outpaint' || request.kind === 'inpaint'
      ? {
          ...request,
          profile: processor.profile,
          ...(processor.selected?.supportsQuality ? { quality } : {}),
          ...(processor.selected?.supportsSizeTier ? { sizeTier } : {}),
        }
      : request;
    void onApply(submitted)
      .catch((caught) => {
        setError(caught instanceof Error ? caught.message : `${imageCanvasToolLabel(kind)}失败`);
      })
      .finally(() => setBusy(false));
  };

  return (
    <details className="inspector-edit-details">
      <summary
        className="inspector-tool"
        onClick={(event) => event.stopPropagation()}
        onPointerDown={(event) => event.stopPropagation()}
        title="扩图、擦除、抠图、超分和增强"
      >
        <Icon n="eraser" s={14} />
        <span>修图</span>
      </summary>
      <form
        className="inspector-details-panel inspector-grid-panel"
        onPointerDown={(event) => event.stopPropagation()}
        onSubmit={submit}
      >
        <div className="inspector-head">
          <div className="kicker">生成式修图</div>
          <div className="title">原图保留，结果生成在旁边</div>
        </div>
        <div className="inspector-body">
          <label className="field">
            <span className="field-label">工具</span>
            <select
              className="field-input"
              disabled={busy || disabled}
              onChange={(event) => selectKind(event.currentTarget.value as ImageCanvasToolKind)}
              value={kind}
            >
              <option value="outpaint">扩图</option>
              <option value="inpaint">擦除</option>
              <option value="cutout">抠图</option>
              <option value="upscale">超分</option>
              <option value="enhance">增强</option>
            </select>
          </label>
          {request.kind === 'outpaint' && (
            <>
              <NumberField
                disabled={Boolean(busy || disabled)}
                label="左"
                onChange={(left) => setRequest({ ...request, left })}
                value={request.left}
              />
              <NumberField
                disabled={Boolean(busy || disabled)}
                label="上"
                onChange={(top) => setRequest({ ...request, top })}
                value={request.top}
              />
              <NumberField
                disabled={Boolean(busy || disabled)}
                label="右"
                onChange={(right) => setRequest({ ...request, right })}
                value={request.right}
              />
              <NumberField
                disabled={Boolean(busy || disabled)}
                label="下"
                onChange={(bottom) => setRequest({ ...request, bottom })}
                value={request.bottom}
              />
            </>
          )}
          {request.kind === 'inpaint' && (
            <>
              <NumberField
                disabled={Boolean(busy || disabled)}
                label="X"
                onChange={(x) => setRequest({ ...request, x })}
                value={request.x}
              />
              <NumberField
                disabled={Boolean(busy || disabled)}
                label="Y"
                onChange={(y) => setRequest({ ...request, y })}
                value={request.y}
              />
              <NumberField
                disabled={Boolean(busy || disabled)}
                label="宽"
                onChange={(width) => setRequest({ ...request, width })}
                value={request.width}
              />
              <NumberField
                disabled={Boolean(busy || disabled)}
                label="高"
                onChange={(height) => setRequest({ ...request, height })}
                value={request.height}
              />
            </>
          )}
          {(request.kind === 'outpaint' || request.kind === 'inpaint') && (
            <>
              <label className="field">
                <span className="field-label">模型</span>
                <select
                  className="field-input"
                  disabled={busy || disabled || processor.loading || Boolean(processor.error)}
                  onChange={(event) => processor.setProfile(event.currentTarget.value)}
                  value={processor.profile}
                >
                  {processor.loading && <option value="">读取中…</option>}
                  {processor.profiles.map((item) => <option key={item.name} value={item.name}>{item.name}</option>)}
                </select>
              </label>
              {(processor.selected?.supportsQuality || processor.selected?.supportsSizeTier) && (
                <>
                  {processor.selected.supportsQuality && (
                    <label className="field">
                      <span className="field-label">质量</span>
                      <select className="field-input" onChange={(event) => setQuality(event.currentTarget.value as 'low' | 'medium' | 'high')} value={quality}>
                        <option value="low">Low</option>
                        <option value="medium">Medium</option>
                        <option value="high">High</option>
                      </select>
                    </label>
                  )}
                  {processor.selected.supportsSizeTier && (
                    <label className="field">
                      <span className="field-label">尺寸</span>
                      <select className="field-input" onChange={(event) => setSizeTier(event.currentTarget.value as '1k' | '2k' | '4k')} value={sizeTier}>
                        <option value="1k">1K</option>
                        <option value="2k">2K</option>
                        <option value="4k">4K</option>
                      </select>
                    </label>
                  )}
                </>
              )}
              <label className="field">
                <span className="field-label">提示词</span>
                <textarea
                  className="field-input field-area"
                  disabled={busy || disabled}
                  onChange={(event) => setRequest({ ...request, prompt: event.currentTarget.value })}
                  rows={4}
                  value={request.prompt}
                />
              </label>
            </>
          )}
          {request.kind === 'upscale' && (
            <label className="field">
              <span className="field-label">倍率</span>
              <select
                className="field-input"
                disabled={busy || disabled}
                onChange={(event) => setRequest({
                  kind: 'upscale',
                  scale: Number(event.currentTarget.value) as 2 | 4,
                })}
                value={request.scale}
              >
                <option value={2}>2×</option>
                <option value={4}>4×</option>
              </select>
            </label>
          )}
          {(error || processor.error) && <div className="inspector-error">{error || processor.error}</div>}
          <button
            className="inspector-tool inspector-tool-primary"
            disabled={
              busy ||
              disabled ||
              ((request.kind === 'outpaint' || request.kind === 'inpaint') && (
                processor.loading || Boolean(processor.error) || !processor.profile
              ))
            }
            type="submit"
          >
            {busy ? '处理中…' : `开始${imageCanvasToolLabel(kind)}`}
          </button>
        </div>
      </form>
    </details>
  );
}

function NumberField({
  disabled,
  label,
  onChange,
  value,
}: {
  disabled: boolean;
  label: string;
  onChange: (value: number) => void;
  value: number;
}) {
  return (
    <label className="field">
      <span className="field-label">{label}</span>
      <input
        className="field-input"
        disabled={disabled}
        min={0}
        onChange={(event) => onChange(Number(event.currentTarget.value))}
        type="number"
        value={value}
      />
    </label>
  );
}
