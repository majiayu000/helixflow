import { useEffect, useRef, useState, type CSSProperties, type PointerEvent as ReactPointerEvent } from 'react';
import { htmlToMarkdown, markdownToHtml } from '../../text-card-markdown';
import {
  artifactsForNode,
  fetchArtifactBlob,
  fetchWorkspaceUploadContent,
  resolveGridSplitSource,
} from '../../grid-split';
import {
  DEFAULT_IMAGE_EDIT_PROMPTS,
  punchEraseMask,
  type ImageGenerationOptions,
} from '../../image-canvas-tools';
import { useImageProcessingProfile } from '../../use-image-processing-profile';
import type { WorkbenchState } from '../../types';
import { graphNodeHeight, graphNodeWidth, type ViewState } from '../graph-canvas-navigation';
import { flowAboveCenterStyle, flowBelowCenterStyle, flowCoverStyle } from './overlay-anchor';

type Pad = { left: number; top: number; right: number; bottom: number };
type Handle = 'n' | 's' | 'e' | 'w' | 'ne' | 'nw' | 'se' | 'sw';

export function OutpaintFrame({
  node,
  outputs,
  params,
  view,
  workspaceId,
  onCancel,
  onConfirm,
}: {
  node: WorkbenchState['graph']['nodes'][number];
  outputs: WorkbenchState['outputs'] | undefined;
  params: unknown;
  view: ViewState;
  workspaceId: string;
  onCancel: () => void;
  onConfirm: (pad: Pad, options: ImageGenerationOptions & { prompt: string }) => void;
}) {
  const [natural, setNatural] = useState({ width: 0, height: 0 });
  const [pad, setPad] = useState<Pad>({ left: 48, top: 48, right: 48, bottom: 48 });
  const [prompt, setPrompt] = useState<string>(DEFAULT_IMAGE_EDIT_PROMPTS.outpaint);
  const [quality, setQuality] = useState<'low' | 'medium' | 'high'>('low');
  const [sizeTier, setSizeTier] = useState<'1k' | '2k' | '4k'>('1k');
  const processor = useImageProcessingProfile(workspaceId, 'outpaint');
  const drag = useRef<{ handle: Handle; startX: number; startY: number; origin: Pad } | null>(null);
  const width = graphNodeWidth(node);
  const height = graphNodeHeight(node);
  const scaleX = natural.width ? width / natural.width : 1;
  const scaleY = natural.height ? height / natural.height : 1;

  useEffect(() => {
    const source = resolveGridSplitSource({
      nodeType: node.nodeType,
      params,
      artifacts: artifactsForNode(outputs, node.id),
    });
    if (!source) return;
    let cancelled = false;
    const blobPromise = source.kind === 'upload'
      ? fetchWorkspaceUploadContent(workspaceId, source.uploadId)
      : fetchArtifactBlob(source.artifactId);
    void blobPromise.then(async (blob) => {
      const bitmap = await createImageBitmap(blob);
      if (cancelled) {
        bitmap.close();
        return;
      }
      setNatural({ width: bitmap.width, height: bitmap.height });
      bitmap.close();
    });
    return () => {
      cancelled = true;
    };
  }, [node.id, node.nodeType, outputs, params, workspaceId]);

  const left = node.position.x - pad.left * scaleX;
  const top = node.position.y - pad.top * scaleY;
  const frameW = width + (pad.left + pad.right) * scaleX;
  const frameH = height + (pad.top + pad.bottom) * scaleY;

  const move = (event: ReactPointerEvent<HTMLDivElement>) => {
    if (!drag.current || !natural.width) return;
    const box = event.currentTarget.getBoundingClientRect();
    const zoomX = frameW > 0 ? box.width / frameW : view.z;
    const zoomY = frameH > 0 ? box.height / frameH : view.z;
    const dx = (event.clientX - drag.current.startX) / (scaleX * zoomX);
    const dy = (event.clientY - drag.current.startY) / (scaleY * zoomY);
    const next = { ...drag.current.origin };
    const handle = drag.current.handle;
    if (handle.includes('w')) next.left = Math.max(0, Math.round(drag.current.origin.left - dx));
    if (handle.includes('e')) next.right = Math.max(0, Math.round(drag.current.origin.right + dx));
    if (handle.includes('n')) next.top = Math.max(0, Math.round(drag.current.origin.top - dy));
    if (handle.includes('s')) next.bottom = Math.max(0, Math.round(drag.current.origin.bottom + dy));
    setPad(next);
  };

  return (
    <div
      className="canvas-outpaint"
      onPointerDown={(event) => event.stopPropagation()}
      onPointerMove={move}
      onPointerUp={() => { drag.current = null; }}
      style={{ left, top, width: frameW, height: frameH } as CSSProperties}
    >
      {(['nw', 'n', 'ne', 'w', 'e', 'sw', 's', 'se'] as Handle[]).map((handle) => (
        <span
          className={`canvas-outpaint-h canvas-outpaint-h--${handle}`}
          key={handle}
          onPointerDown={(event) => {
            event.currentTarget.setPointerCapture(event.pointerId);
            drag.current = { handle, startX: event.clientX, startY: event.clientY, origin: pad };
          }}
        />
      ))}
      <div className="canvas-image-edit-panel">
        <ImageGenerationControls
          error={processor.error}
          loading={processor.loading}
          onProfileChange={processor.setProfile}
          onPromptChange={setPrompt}
          onQualityChange={setQuality}
          onSizeTierChange={setSizeTier}
          profile={processor.profile}
          profiles={processor.profiles}
          prompt={prompt}
          quality={quality}
          sizeTier={sizeTier}
          supportsQuality={Boolean(processor.selected?.supportsQuality)}
          supportsSizeTier={Boolean(processor.selected?.supportsSizeTier)}
        />
        <div className="canvas-image-edit-actions">
          <button onClick={onCancel} type="button">取消</button>
          <button
            className="canvas-outpaint-run"
            disabled={
              processor.loading ||
              Boolean(processor.error) ||
              !processor.profile ||
              !prompt.trim() ||
              pad.left + pad.top + pad.right + pad.bottom === 0
            }
            onClick={() => onConfirm(pad, {
              profile: processor.profile,
              prompt,
              ...(processor.selected?.supportsQuality ? { quality } : {}),
              ...(processor.selected?.supportsSizeTier ? { sizeTier } : {}),
            })}
            type="button"
          >
            生成扩图
          </button>
        </div>
      </div>
    </div>
  );
}

export function EraseStage({
  intent = 'erase',
  node,
  outputs,
  params,
  view: _view,
  workspaceId,
  onCancel,
  onConfirm,
}: {
  intent?: 'erase' | 'redraw';
  node: WorkbenchState['graph']['nodes'][number];
  outputs: WorkbenchState['outputs'] | undefined;
  params: unknown;
  view: ViewState;
  workspaceId: string;
  onCancel: () => void;
  onConfirm: (maskBlob: Blob, options: ImageGenerationOptions & { prompt: string }) => void;
}) {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const [url, setUrl] = useState<string | null>(null);
  const [tool, setTool] = useState<'brush' | 'rect'>('brush');
  const [hasMask, setHasMask] = useState(false);
  const [prompt, setPrompt] = useState<string>(
    intent === 'redraw' ? DEFAULT_IMAGE_EDIT_PROMPTS.redraw : DEFAULT_IMAGE_EDIT_PROMPTS.inpaint,
  );
  const [quality, setQuality] = useState<'low' | 'medium' | 'high'>('low');
  const [sizeTier, setSizeTier] = useState<'1k' | '2k' | '4k'>('1k');
  const processor = useImageProcessingProfile(workspaceId, 'inpaint');
  const [natural, setNatural] = useState({ width: 0, height: 0 });
  const maskRef = useRef<Uint8Array | null>(null);
  const drag = useRef<{ x: number; y: number } | null>(null);
  const cover = flowCoverStyle(node);

  useEffect(() => {
    const source = resolveGridSplitSource({
      nodeType: node.nodeType,
      params,
      artifacts: artifactsForNode(outputs, node.id),
    });
    if (!source) return;
    let cancelled = false;
    let objectUrl: string | null = null;
    const blobPromise = source.kind === 'upload'
      ? fetchWorkspaceUploadContent(workspaceId, source.uploadId)
      : fetchArtifactBlob(source.artifactId);
    void blobPromise.then(async (blob) => {
      const bitmap = await createImageBitmap(blob);
      if (cancelled) {
        bitmap.close();
        return;
      }
      setNatural({ width: bitmap.width, height: bitmap.height });
      maskRef.current = new Uint8Array(bitmap.width * bitmap.height);
      objectUrl = URL.createObjectURL(blob);
      setUrl(objectUrl);
      bitmap.close();
    });
    return () => {
      cancelled = true;
      if (objectUrl) URL.revokeObjectURL(objectUrl);
    };
  }, [node.id, node.nodeType, outputs, params, workspaceId]);

  const local = (event: ReactPointerEvent<HTMLCanvasElement>) => {
    const box = event.currentTarget.getBoundingClientRect();
    return {
      x: ((event.clientX - box.left) / box.width) * natural.width,
      y: ((event.clientY - box.top) / box.height) * natural.height,
    };
  };

  const paintCircle = (cx: number, cy: number) => {
    const mask = maskRef.current;
    if (!mask || !natural.width) return;
    const radius = Math.max(6, Math.round(Math.min(natural.width, natural.height) * 0.03));
    const r2 = radius * radius;
    const x0 = Math.max(0, Math.floor(cx - radius));
    const x1 = Math.min(natural.width - 1, Math.ceil(cx + radius));
    const y0 = Math.max(0, Math.floor(cy - radius));
    const y1 = Math.min(natural.height - 1, Math.ceil(cy + radius));
    for (let y = y0; y <= y1; y += 1) {
      for (let x = x0; x <= x1; x += 1) {
        const dx = x - cx;
        const dy = y - cy;
        if (dx * dx + dy * dy <= r2) mask[y * natural.width + x] = 1;
      }
    }
    setHasMask(true);
  };

  const drawOverlay = (preview?: { x: number; y: number; w: number; h: number }) => {
    const canvas = canvasRef.current;
    const mask = maskRef.current;
    if (!canvas || !mask || !natural.width) return;
    canvas.width = natural.width;
    canvas.height = natural.height;
    const context = canvas.getContext('2d');
    if (!context) return;
    context.clearRect(0, 0, natural.width, natural.height);
    const image = context.createImageData(natural.width, natural.height);
    for (let index = 0; index < mask.length; index += 1) {
      if (!mask[index]) continue;
      const pixel = index * 4;
      image.data[pixel] = 255;
      image.data[pixel + 1] = 70;
      image.data[pixel + 2] = 70;
      image.data[pixel + 3] = 110;
    }
    context.putImageData(image, 0, 0);
    if (preview) {
      context.fillStyle = 'rgba(255,70,70,.35)';
      context.fillRect(preview.x, preview.y, preview.w, preview.h);
    }
  };

  return (
    <div
      className="canvas-erase nodrag nopan"
      onPointerDown={(event) => event.stopPropagation()}
      style={cover}
    >
      {url ? <img alt="" src={url} /> : <div className="pixel-crop-empty">正在读取原图…</div>}
      <canvas
        ref={canvasRef}
        onPointerDown={(event) => {
          if (!natural.width) return;
          event.currentTarget.setPointerCapture(event.pointerId);
          const at = local(event);
          drag.current = at;
          if (tool === 'brush') {
            paintCircle(at.x, at.y);
            drawOverlay();
          }
        }}
        onPointerMove={(event) => {
          if (!drag.current || !natural.width) return;
          const at = local(event);
          if (tool === 'brush') {
            paintCircle(at.x, at.y);
            drawOverlay();
            return;
          }
          drawOverlay({
            x: Math.min(drag.current.x, at.x),
            y: Math.min(drag.current.y, at.y),
            w: Math.abs(at.x - drag.current.x),
            h: Math.abs(at.y - drag.current.y),
          });
        }}
        onPointerUp={(event) => {
          if (tool === 'rect' && drag.current && natural.width) {
            const at = local(event);
            const mask = maskRef.current;
            if (mask) {
              const x0 = Math.max(0, Math.floor(Math.min(drag.current.x, at.x)));
              const x1 = Math.min(natural.width - 1, Math.ceil(Math.max(drag.current.x, at.x)));
              const y0 = Math.max(0, Math.floor(Math.min(drag.current.y, at.y)));
              const y1 = Math.min(natural.height - 1, Math.ceil(Math.max(drag.current.y, at.y)));
              for (let y = y0; y <= y1; y += 1) {
                mask.fill(1, y * natural.width + x0, y * natural.width + x1 + 1);
              }
              setHasMask(true);
            }
            drawOverlay();
          }
          drag.current = null;
        }}
      />
      <div className="canvas-image-edit-panel">
        <div className="canvas-image-edit-tools">
          <button className={tool === 'brush' ? 'on' : ''} onClick={() => setTool('brush')} type="button">笔刷</button>
          <button className={tool === 'rect' ? 'on' : ''} onClick={() => setTool('rect')} type="button">框选</button>
        </div>
        <ImageGenerationControls
          error={processor.error}
          loading={processor.loading}
          onProfileChange={processor.setProfile}
          onPromptChange={setPrompt}
          onQualityChange={setQuality}
          onSizeTierChange={setSizeTier}
          profile={processor.profile}
          profiles={processor.profiles}
          prompt={prompt}
          quality={quality}
          sizeTier={sizeTier}
          supportsQuality={Boolean(processor.selected?.supportsQuality)}
          supportsSizeTier={Boolean(processor.selected?.supportsSizeTier)}
        />
        <div className="canvas-image-edit-actions">
          <button onClick={onCancel} type="button">取消</button>
          <button
            className="canvas-outpaint-run"
            disabled={
              !hasMask ||
              processor.loading ||
              Boolean(processor.error) ||
              !processor.profile ||
              !prompt.trim()
            }
            onClick={() => {
              const mask = maskRef.current;
              if (!mask || !mask.some(Boolean) || !url) return;
              const options = {
                profile: processor.profile,
                prompt,
                ...(processor.selected?.supportsQuality ? { quality } : {}),
                ...(processor.selected?.supportsSizeTier ? { sizeTier } : {}),
              };
              void fetch(url)
                .then((response) => response.blob())
                .then((blob) => punchEraseMask(blob, mask, natural.width, natural.height))
                .then((blob) => onConfirm(blob, options));
            }}
            type="button"
          >
            生成{intent === 'redraw' ? '重绘' : '擦除'}
          </button>
        </div>
      </div>
    </div>
  );
}

function ImageGenerationControls({
  error,
  loading,
  onProfileChange,
  onPromptChange,
  onQualityChange,
  onSizeTierChange,
  profile,
  profiles,
  prompt,
  quality,
  sizeTier,
  supportsQuality,
  supportsSizeTier,
}: {
  error: string | null;
  loading: boolean;
  onProfileChange: (value: string) => void;
  onPromptChange: (value: string) => void;
  onQualityChange: (value: 'low' | 'medium' | 'high') => void;
  onSizeTierChange: (value: '1k' | '2k' | '4k') => void;
  profile: string;
  profiles: Array<{ name: string; supportsQuality: boolean; supportsSizeTier: boolean }>;
  prompt: string;
  quality: 'low' | 'medium' | 'high';
  sizeTier: '1k' | '2k' | '4k';
  supportsQuality: boolean;
  supportsSizeTier: boolean;
}) {
  return (
    <div className="canvas-image-edit-fields">
      <label>
        <span>模型</span>
        <select
          disabled={loading || Boolean(error)}
          onChange={(event) => onProfileChange(event.currentTarget.value)}
          value={profile}
        >
          {loading && <option value="">读取中…</option>}
          {!loading && profiles.length === 0 && <option value="">不可用</option>}
          {profiles.map((item) => <option key={item.name} value={item.name}>{item.name}</option>)}
        </select>
      </label>
      {(supportsQuality || supportsSizeTier) && (
        <div className="canvas-image-edit-options">
          {supportsQuality && (
          <label>
            <span>质量</span>
            <select
              onChange={(event) => onQualityChange(event.currentTarget.value as 'low' | 'medium' | 'high')}
              value={quality}
            >
              <option value="low">Low</option>
              <option value="medium">Medium</option>
              <option value="high">High</option>
            </select>
          </label>
          )}
          {supportsSizeTier && (
          <label>
            <span>尺寸</span>
            <select
              onChange={(event) => onSizeTierChange(event.currentTarget.value as '1k' | '2k' | '4k')}
              value={sizeTier}
            >
              <option value="1k">1K</option>
              <option value="2k">2K</option>
              <option value="4k">4K</option>
            </select>
          </label>
          )}
        </div>
      )}
      <label>
        <span>修改要求</span>
        <textarea
          onChange={(event) => onPromptChange(event.currentTarget.value)}
          rows={2}
          value={prompt}
        />
      </label>
      {error && <div className="canvas-image-edit-error">{error}</div>}
    </div>
  );
}

const TEXT_FORMATS = [
  { command: 'formatBlock', value: 'h1', label: 'H1' },
  { command: 'formatBlock', value: 'h2', label: 'H2' },
  { command: 'formatBlock', value: 'h3', label: 'H3' },
  { command: 'formatBlock', value: 'p', label: '¶' },
  { command: 'bold', label: 'B' },
  { command: 'italic', label: 'I' },
  { command: 'insertUnorderedList', label: '•' },
  { command: 'insertOrderedList', label: '1.' },
] as const;

export function TextCardEditor({
  node,
  view: _view,
  value,
  onSave,
}: {
  node: WorkbenchState['graph']['nodes'][number];
  view: ViewState;
  value: string;
  onSave: (text: string) => void;
}) {
  const editorRef = useRef<HTMLDivElement | null>(null);
  const cover = flowCoverStyle(node);
  useEffect(() => {
    editorRef.current?.focus();
  }, [node.id]);
  const commit = () => {
    const next = htmlToMarkdown(editorRef.current?.innerHTML ?? '');
    if (next !== value) onSave(next);
  };
  return (
    <>
      <div
        className="canvas-text-toolbar nodrag nopan"
        onMouseDown={(event) => event.preventDefault()}
        onPointerDown={(event) => event.stopPropagation()}
        style={flowAboveCenterStyle(node, 10)}
      >
        {TEXT_FORMATS.map((format) => (
          <button
            aria-label={format.label}
            key={format.label}
            onClick={() => {
              editorRef.current?.focus();
              if (typeof document !== 'undefined') {
                document.execCommand(format.command, false, 'value' in format ? format.value : undefined);
              }
            }}
            type="button"
          >
            {format.label}
          </button>
        ))}
      </div>
      <div
        aria-label="文本"
        className="canvas-text-editor nodrag nopan"
        contentEditable
        data-placeholder="双击开始编辑..."
        dangerouslySetInnerHTML={{ __html: markdownToHtml(value) }}
        onBlur={commit}
        onKeyDown={(event) => {
          if (event.key === 'Escape') {
            event.currentTarget.blur();
          }
        }}
        onPointerDown={(event) => event.stopPropagation()}
        ref={editorRef}
        role="textbox"
        style={cover}
        suppressContentEditableWarning
      />
    </>
  );
}

export function CameraToolPanel({
  kind,
  node,
  onCancel,
  onRun,
  view: _view,
}: {
  kind: 'relight' | 'multi-angle';
  node: WorkbenchState['graph']['nodes'][number];
  onCancel: () => void;
  onRun: (prompt: string) => void;
  view: ViewState;
}) {
  const [dir, setDir] = useState('左');
  const [brightness, setBrightness] = useState(50);
  const [temp, setTemp] = useState(5600);
  const [yaw, setYaw] = useState(0);
  const [pitch, setPitch] = useState(0);
  const [distance, setDistance] = useState(50);
  const prompt = kind === 'relight'
    ? `Relight this image. Key light from the ${dir} at ${brightness}% brightness and ${temp}K. Keep the subject, pose, and background.`
    : `Rephotograph this scene. Camera yaw ${yaw}°, pitch ${pitch}°, distance ${distance}%. Keep identity and wardrobe.`;
  return (
    <div
      className="canvas-tool-panel nodrag nopan"
      style={{ ...flowBelowCenterStyle(node, 12), transform: 'translateX(-50%)' } as CSSProperties}
      onPointerDown={(event) => event.stopPropagation()}
    >
      <strong>{kind === 'relight' ? '用提示词调光' : '用提示词换角度'}</strong>
      {kind === 'relight' ? (
        <>
          <div className="canvas-tool-panel-dirs">
            {['左', '顶', '右', '前', '底', '后'].map((item) => (
              <button className={dir === item ? 'on' : undefined} key={item} onClick={() => setDir(item)} type="button">
                {item}
              </button>
            ))}
          </div>
          <label>
            亮度 {brightness}%
            <input max={100} min={0} onChange={(event) => setBrightness(Number(event.target.value))} type="range" value={brightness} />
          </label>
          <label>
            色温 {temp}K
            <input max={9000} min={2800} onChange={(event) => setTemp(Number(event.target.value))} step={100} type="range" value={temp} />
          </label>
        </>
      ) : (
        <>
          <label>
            旋转 {yaw}°
            <input max={180} min={-180} onChange={(event) => setYaw(Number(event.target.value))} type="range" value={yaw} />
          </label>
          <label>
            倾斜 {pitch}°
            <input max={60} min={-60} onChange={(event) => setPitch(Number(event.target.value))} type="range" value={pitch} />
          </label>
          <label>
            距离 {distance}%
            <input max={100} min={10} onChange={(event) => setDistance(Number(event.target.value))} type="range" value={distance} />
          </label>
        </>
      )}
      <div className="op-actions">
        <button className="op-cancel" onClick={onCancel} type="button">取消</button>
        <button className="op-run" onClick={() => onRun(prompt)} type="button">生成</button>
      </div>
    </div>
  );
}
