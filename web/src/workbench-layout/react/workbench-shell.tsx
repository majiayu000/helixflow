import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent,
  type PointerEvent,
  type ReactNode,
} from 'react';
import type { WorkbenchPaneBindings } from '../../workbench-pane-registry';
import {
  WORKBENCH_PANE_DEFINITIONS,
  WORKBENCH_ZONE_CONSTRAINTS,
} from '../defaults';
import { resolveWorkbenchDropTarget } from '../dom-geometry';
import {
  beginWorkbenchDrag,
  updateWorkbenchDrag,
  workbenchDragCommit,
  type WorkbenchDragSession,
} from '../drag-controller';
import {
  beginWorkbenchResize,
  workbenchResizeSize,
  type ResizableWorkbenchZoneId,
  type WorkbenchResizeSession,
} from '../resize-controller';
import { useWorkbenchLayoutStore } from '../store';
import type {
  WorkbenchLayoutCommand,
  WorkbenchLayoutDocument,
  WorkbenchViewContainerState,
  WorkbenchZoneId,
} from '../types';

type WorkbenchShellProps = {
  panes: WorkbenchPaneBindings;
  overlays?: ReactNode;
};

type PreviewSizes = Partial<Record<ResizableWorkbenchZoneId, number>>;

export function WorkbenchShell({ panes, overlays }: WorkbenchShellProps) {
  const rootRef = useRef<HTMLDivElement | null>(null);
  const compact = useCompactWorkbench();
  const document = useWorkbenchLayoutStore((state) => state.document);
  const dispatch = useWorkbenchLayoutStore((state) => state.dispatch);
  const [previewSizes, setPreviewSizes] = useState<PreviewSizes>({});
  const [dragSession, setDragSession] = useState<WorkbenchDragSession | null>(null);
  const [compactOpenZone, setCompactOpenZone] = useState<ResizableWorkbenchZoneId | null>(null);

  useEffect(() => {
    if (!dragSession) return;
    const cancel = () => setDragSession(null);
    const onKeyDown = (event: globalThis.KeyboardEvent) => {
      if (event.key === 'Escape') cancel();
    };
    globalThis.addEventListener?.('blur', cancel);
    globalThis.addEventListener?.('keydown', onKeyDown);
    return () => {
      globalThis.removeEventListener?.('blur', cancel);
      globalThis.removeEventListener?.('keydown', onKeyDown);
    };
  }, [dragSession]);

  const availableContainers = useMemo(
    () => availableContainerIds(document, panes),
    [document, panes],
  );
  const showPrimary = zoneHasAvailableContainer(document, 'primarySidebar', availableContainers);
  const showSecondary = zoneHasAvailableContainer(document, 'secondarySidebar', availableContainers);
  const showPanel = zoneHasAvailableContainer(document, 'panel', availableContainers);
  const primarySize = previewSizes.primarySidebar ?? document.zones.primarySidebar.sizePx ?? 420;
  const secondarySize = previewSizes.secondarySidebar ?? document.zones.secondarySidebar.sizePx ?? 420;
  const panelSize = previewSizes.panel ?? document.zones.panel.sizePx ?? 240;
  const primaryExpanded = compact
    ? compactOpenZone === 'primarySidebar'
    : document.zones.primarySidebar.visible;
  const secondaryExpanded = compact
    ? compactOpenZone === 'secondarySidebar'
    : document.zones.secondarySidebar.visible;
  const panelExpanded = compact
    ? compactOpenZone === 'panel'
    : document.zones.panel.visible;
  const primaryInset = showPrimary && primaryExpanded ? primarySize + 24 : 14;
  const secondaryInset = showSecondary && secondaryExpanded ? secondarySize + 24 : 14;
  const panelInset = showPanel && panelExpanded ? panelSize + 24 : 14;
  const shellStyle = {
    '--workbench-primary-inset': `${primaryInset}px`,
    '--workbench-secondary-inset': `${secondaryInset}px`,
    '--workbench-panel-inset': `${panelInset}px`,
    '--workbench-center-shift': `${(primaryInset - secondaryInset) / 2}px`,
  } as CSSProperties;

  const beginDrag = (event: PointerEvent<HTMLDivElement>, containerId: string) => {
    if (event.button !== 0 || isInteractiveTarget(event.target) || compactWorkbench()) return;
    event.currentTarget.setPointerCapture(event.pointerId);
    setDragSession(beginWorkbenchDrag(
      containerId,
      event.pointerId,
      event.clientX,
      event.clientY,
    ));
  };

  const updateDrag = (event: PointerEvent<HTMLDivElement>) => {
    if (!dragSession || dragSession.pointerId !== event.pointerId || !rootRef.current) return;
    const allowedZones = allowedZonesForContainer(document, dragSession.containerId);
    const target = resolveWorkbenchDropTarget(
      rootRef.current,
      event.clientX,
      event.clientY,
      allowedZones,
    );
    setDragSession(updateWorkbenchDrag(
      dragSession,
      event.pointerId,
      event.clientX,
      event.clientY,
      target,
    ));
  };

  const endDrag = (event: PointerEvent<HTMLDivElement>) => {
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    const commit = workbenchDragCommit(dragSession, event.pointerId);
    setDragSession(null);
    if (!commit) return;
    dispatch({
      type: 'container/move',
      containerId: commit.containerId,
      toZoneId: commit.target.zoneId,
      index: commit.target.index,
    });
  };

  const cancelDrag = (event: PointerEvent<HTMLDivElement>) => {
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    setDragSession(null);
  };

  const zoneProps = {
    compact,
    compactOpenZone,
    document,
    panes,
    dispatch,
    dragSession,
    onPointerCancel: cancelDrag,
    onPointerDown: beginDrag,
    onPointerMove: updateDrag,
    onPointerUp: endDrag,
    onCompactToggle: (zoneId: ResizableWorkbenchZoneId) => {
      setCompactOpenZone((current) => current === zoneId ? null : zoneId);
    },
  };

  return (
    <div className="workbench-shell" ref={rootRef} style={shellStyle}>
      {showPrimary && (
        <div
          className={`workbench-drawer-layer workbench-drawer-layer--primary${primaryExpanded ? '' : ' is-collapsed'}`}
          style={{ width: primaryExpanded ? primarySize : 44 }}
        >
          <WorkbenchZone
            {...zoneProps}
            orientation="vertical"
            sizePx={primarySize}
            zoneId="primarySidebar"
          />
          {primaryExpanded && (
            <WorkbenchSash
              currentSizePx={primarySize}
              onCommit={(sizePx) => {
                setPreviewSizes((current) => ({ ...current, primarySidebar: undefined }));
                dispatch({ type: 'zone/resize', zoneId: 'primarySidebar', sizePx });
              }}
              onPreview={(sizePx) => setPreviewSizes((current) => ({ ...current, primarySidebar: sizePx }))}
              zoneId="primarySidebar"
            />
          )}
        </div>
      )}
      <div className="workbench-center">
        <WorkbenchZone
          {...zoneProps}
          orientation="vertical"
          sizePx={null}
          zoneId="editor"
        />
        {showPanel && (
          <div
            className={`workbench-panel-layer${panelExpanded ? '' : ' is-collapsed'}`}
            style={{ height: panelExpanded ? panelSize : 42 }}
          >
            {panelExpanded && (
              <WorkbenchSash
                currentSizePx={panelSize}
                onCommit={(sizePx) => {
                  setPreviewSizes((current) => ({ ...current, panel: undefined }));
                  dispatch({ type: 'zone/resize', zoneId: 'panel', sizePx });
                }}
                onPreview={(sizePx) => setPreviewSizes((current) => ({ ...current, panel: sizePx }))}
                zoneId="panel"
              />
            )}
            <WorkbenchZone
              {...zoneProps}
              orientation="horizontal"
              sizePx={null}
              zoneId="panel"
            />
          </div>
        )}
      </div>
      {showSecondary && (
        <div
          className={`workbench-drawer-layer workbench-drawer-layer--secondary${secondaryExpanded ? '' : ' is-collapsed'}`}
          style={{ width: secondaryExpanded ? secondarySize : 44 }}
        >
          {secondaryExpanded && (
            <WorkbenchSash
              currentSizePx={secondarySize}
              onCommit={(sizePx) => {
                setPreviewSizes((current) => ({ ...current, secondarySidebar: undefined }));
                dispatch({ type: 'zone/resize', zoneId: 'secondarySidebar', sizePx });
              }}
              onPreview={(sizePx) => setPreviewSizes((current) => ({ ...current, secondarySidebar: sizePx }))}
              zoneId="secondarySidebar"
            />
          )}
          <WorkbenchZone
            {...zoneProps}
            orientation="vertical"
            sizePx={secondarySize}
            zoneId="secondarySidebar"
          />
        </div>
      )}
      {dragSession?.active && (
        <WorkbenchDockOverlay
          activeZoneId={dragSession.target?.zoneId ?? null}
          allowedZones={allowedZonesForContainer(document, dragSession.containerId)}
        />
      )}
      {overlays}
    </div>
  );
}

type WorkbenchZoneProps = {
  compact: boolean;
  compactOpenZone: ResizableWorkbenchZoneId | null;
  document: WorkbenchLayoutDocument;
  panes: WorkbenchPaneBindings;
  dispatch: (command: WorkbenchLayoutCommand) => unknown;
  dragSession: WorkbenchDragSession | null;
  orientation: 'horizontal' | 'vertical';
  sizePx: number | null;
  zoneId: WorkbenchZoneId;
  onPointerDown: (event: PointerEvent<HTMLDivElement>, containerId: string) => void;
  onPointerMove: (event: PointerEvent<HTMLDivElement>) => void;
  onPointerUp: (event: PointerEvent<HTMLDivElement>) => void;
  onPointerCancel: (event: PointerEvent<HTMLDivElement>) => void;
  onCompactToggle: (zoneId: ResizableWorkbenchZoneId) => void;
};

function WorkbenchZone({
  compact,
  compactOpenZone,
  document,
  panes,
  dispatch,
  dragSession,
  orientation,
  sizePx,
  zoneId,
  onPointerCancel,
  onPointerDown,
  onPointerMove,
  onPointerUp,
  onCompactToggle,
}: WorkbenchZoneProps) {
  const zone = document.zones[zoneId];
  const containers = zone.containerIds
    .map((id) => document.containers[id])
    .filter((container) => container && container.paneIds.some((id) => panes[id]?.available));
  const hiddenPaneIds = containers.flatMap((container) => container.paneIds).filter(
    (paneId) => panes[paneId]?.available && !document.panes[paneId]?.visible,
  );
  const collapsed = zoneId !== 'editor' && (compact ? compactOpenZone !== zoneId : !zone.visible);
  const style = zoneStyle(zoneId, collapsed, sizePx);

  if (collapsed) {
    return (
      <aside
        className={`workbench-zone-rail workbench-zone-rail--${zoneId}`}
        data-workbench-zone={zoneId}
        style={style}
      >
        <button
          aria-label={`展开${zoneLabel(zoneId)}`}
          onClick={() => compact
            ? onCompactToggle(zoneId as ResizableWorkbenchZoneId)
            : dispatch({ type: 'zone/toggle', zoneId })}
          title={`展开${zoneLabel(zoneId)}`}
          type="button"
        >
          {zoneShortLabel(zoneId)}
        </button>
      </aside>
    );
  }

  return (
    <section
      className={`workbench-zone workbench-zone--${zoneId} workbench-zone--${orientation}`}
      data-workbench-zone={zoneId}
      style={style}
    >
      {zoneId !== 'editor' && hiddenPaneIds.length > 0 && (
        <div className="workbench-zone-restore-actions" aria-label="恢复隐藏面板">
          {hiddenPaneIds.map((paneId) => (
            <button
              aria-label={`显示${WORKBENCH_PANE_DEFINITIONS[paneId].title}`}
              key={paneId}
              onClick={() => dispatch({ type: 'pane/show', paneId })}
              title={`显示 ${WORKBENCH_PANE_DEFINITIONS[paneId].title}`}
              type="button"
            >
              <span aria-hidden="true">+</span>
              {WORKBENCH_PANE_DEFINITIONS[paneId].title}
            </button>
          ))}
        </div>
      )}
      <div className="workbench-zone-containers">
        {containers.map((container, index) => (
          <WorkbenchViewContainer
            container={container}
            containerIndex={index}
            dispatch={dispatch}
            document={document}
            dragging={dragSession?.containerId === container.id && dragSession.active}
            key={container.id}
            onPointerCancel={onPointerCancel}
            onPointerDown={onPointerDown}
            onPointerMove={onPointerMove}
            onPointerUp={onPointerUp}
            panes={panes}
          />
        ))}
      </div>
    </section>
  );
}

type ViewContainerProps = {
  container: WorkbenchViewContainerState;
  containerIndex: number;
  dispatch: (command: WorkbenchLayoutCommand) => unknown;
  document: WorkbenchLayoutDocument;
  dragging: boolean;
  panes: WorkbenchPaneBindings;
  onPointerDown: (event: PointerEvent<HTMLDivElement>, containerId: string) => void;
  onPointerMove: (event: PointerEvent<HTMLDivElement>) => void;
  onPointerUp: (event: PointerEvent<HTMLDivElement>) => void;
  onPointerCancel: (event: PointerEvent<HTMLDivElement>) => void;
};

function WorkbenchViewContainer({
  container,
  containerIndex,
  dispatch,
  document,
  dragging,
  panes,
  onPointerCancel,
  onPointerDown,
  onPointerMove,
  onPointerUp,
}: ViewContainerProps) {
  const visiblePanes = container.paneIds.filter((id) => document.panes[id]?.visible && panes[id]?.available);
  if (visiblePanes.length === 0) return null;

  return (
    <div
      className={`workbench-view-container${dragging ? ' is-dragging' : ''}`}
      data-container-id={container.id}
    >
      {visiblePanes.map((paneId) => {
        const placement = document.panes[paneId];
        const definition = WORKBENCH_PANE_DEFINITIONS[paneId];
        const binding = panes[paneId];
        return (
          <section
            className={`workbench-pane${placement.collapsed ? ' is-collapsed' : ''}`}
            data-pane-id={paneId}
            key={paneId}
          >
            {paneId !== 'canvas' && (
              <div
                className="workbench-pane-header"
                onDoubleClick={(event) => {
                  if (!isInteractiveTarget(event.target) && definition.canCollapse) {
                    dispatch({ type: 'pane/toggleCollapsed', paneId });
                  }
                }}
                onPointerCancel={onPointerCancel}
                onPointerDown={(event) => onPointerDown(event, container.id)}
                onPointerMove={onPointerMove}
                onPointerUp={onPointerUp}
                title="拖拽到工作台区域，双击折叠"
              >
                <span className="workbench-pane-grip" aria-hidden="true">⠿</span>
                <button
                  aria-expanded={!placement.collapsed}
                  className="workbench-pane-toggle"
                  disabled={!definition.canCollapse}
                  onClick={() => dispatch({ type: 'pane/toggleCollapsed', paneId })}
                  type="button"
                >
                  <span aria-hidden="true">{placement.collapsed ? '›' : '⌄'}</span>
                  <span>{definition.title}</span>
                  {binding.badge !== undefined && <span className="workbench-pane-badge">{binding.badge}</span>}
                </button>
                <div className="workbench-pane-actions">
                  <details className="workbench-pane-menu">
                    <summary aria-label={`${definition.title}更多操作`} title="更多操作">•••</summary>
                    <div className="workbench-pane-menu-popover">
                      <button
                        aria-label={`前移${definition.title}`}
                        disabled={containerIndex === 0}
                        onClick={() => dispatch({ type: 'container/reorder', containerId: container.id, index: containerIndex - 1 })}
                        type="button"
                      >上移面板</button>
                      <button
                        aria-label={`后移${definition.title}`}
                        disabled={containerIndex >= document.zones[container.zoneId].containerIds.length - 1}
                        onClick={() => dispatch({ type: 'container/reorder', containerId: container.id, index: containerIndex + 1 })}
                        type="button"
                      >下移面板</button>
                      <label className="workbench-pane-move-label">
                        <span>移动到</span>
                        <select
                          aria-label={`移动${definition.title}`}
                          onChange={(event) => {
                            const toZoneId = event.currentTarget.value as WorkbenchZoneId;
                            if (toZoneId) dispatch({ type: 'container/move', containerId: container.id, toZoneId });
                            event.currentTarget.value = '';
                          }}
                          defaultValue=""
                        >
                          <option value="" disabled>选择区域</option>
                          {definition.allowedZones.filter((id) => id !== container.zoneId).map((id) => (
                            <option key={id} value={id}>{zoneLabel(id)}</option>
                          ))}
                        </select>
                      </label>
                      {definition.canHide && (
                        <button
                          aria-label={`隐藏${definition.title}`}
                          onClick={() => dispatch({ type: 'pane/hide', paneId })}
                          type="button"
                        >隐藏面板</button>
                      )}
                    </div>
                  </details>
                  {container.zoneId !== 'editor' && containerIndex === 0 && (
                    <button
                      aria-label={`收起${zoneLabel(container.zoneId)}`}
                      onClick={() => dispatch({ type: 'zone/toggle', zoneId: container.zoneId })}
                      title={`收起${zoneLabel(container.zoneId)}`}
                      type="button"
                    >{zoneCollapseGlyph(container.zoneId)}</button>
                  )}
                </div>
              </div>
            )}
            <div className="workbench-pane-content" hidden={placement.collapsed}>
              {binding.content}
            </div>
          </section>
        );
      })}
    </div>
  );
}

type WorkbenchSashProps = {
  currentSizePx: number;
  zoneId: ResizableWorkbenchZoneId;
  onPreview: (sizePx: number) => void;
  onCommit: (sizePx: number) => void;
};

function WorkbenchSash({ currentSizePx, zoneId, onPreview, onCommit }: WorkbenchSashProps) {
  const sessionRef = useRef<WorkbenchResizeSession | null>(null);
  const constraint = WORKBENCH_ZONE_CONSTRAINTS[zoneId];
  const orientation = zoneId === 'panel' ? 'horizontal' : 'vertical';

  const endResize = (event: PointerEvent<HTMLDivElement>, commit: boolean) => {
    const session = sessionRef.current;
    if (!session || session.pointerId !== event.pointerId) return;
    const size = workbenchResizeSize(session, event.pointerId, event.clientX, event.clientY);
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    sessionRef.current = null;
    if (commit && size !== null) onCommit(size);
    else onPreview(currentSizePx);
  };

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const step = event.shiftKey ? 40 : 10;
    const decrease = event.key === (orientation === 'vertical' ? 'ArrowLeft' : 'ArrowDown');
    const increase = event.key === (orientation === 'vertical' ? 'ArrowRight' : 'ArrowUp');
    if (!decrease && !increase) return;
    event.preventDefault();
    const direction = zoneId === 'secondarySidebar' && orientation === 'vertical' ? -1 : 1;
    onCommit(currentSizePx + (increase ? step : -step) * direction);
  };

  return (
    <div
      aria-label={`调整${zoneLabel(zoneId)}大小`}
      aria-orientation={orientation}
      aria-valuemax={constraint.maxSizePx}
      aria-valuemin={constraint.minSizePx}
      aria-valuenow={currentSizePx}
      className={`workbench-sash workbench-sash--${orientation}`}
      onDoubleClick={() => onCommit(constraint.defaultSizePx)}
      onKeyDown={onKeyDown}
      onPointerCancel={(event) => endResize(event, false)}
      onPointerDown={(event) => {
        if (event.button !== 0) return;
        sessionRef.current = beginWorkbenchResize(
          zoneId,
          event.pointerId,
          event.clientX,
          event.clientY,
          currentSizePx,
        );
        event.currentTarget.setPointerCapture(event.pointerId);
      }}
      onPointerMove={(event) => {
        const session = sessionRef.current;
        if (!session) return;
        const size = workbenchResizeSize(session, event.pointerId, event.clientX, event.clientY);
        if (size !== null) onPreview(size);
      }}
      onPointerUp={(event) => endResize(event, true)}
      role="separator"
      tabIndex={0}
    />
  );
}

function WorkbenchDockOverlay({
  activeZoneId,
  allowedZones,
}: {
  activeZoneId: WorkbenchZoneId | null;
  allowedZones: readonly WorkbenchZoneId[];
}) {
  return (
    <div className="workbench-dock-overlay" aria-label="工作台停靠位置">
      {allowedZones.map((zoneId) => (
        <div
          className={`workbench-dock-target workbench-dock-target--${zoneId}${activeZoneId === zoneId ? ' is-active' : ''}`}
          data-workbench-drop-zone={zoneId}
          key={zoneId}
        >
          <span>{zoneLabel(zoneId)}</span>
        </div>
      ))}
    </div>
  );
}

function availableContainerIds(
  document: WorkbenchLayoutDocument,
  panes: WorkbenchPaneBindings,
): Set<string> {
  return new Set(Object.values(document.containers)
    .filter((container) => container.paneIds.some((id) => panes[id]?.available))
    .map((container) => container.id));
}

function zoneHasAvailableContainer(
  document: WorkbenchLayoutDocument,
  zoneId: WorkbenchZoneId,
  available: Set<string>,
): boolean {
  return document.zones[zoneId].containerIds.some((id) => available.has(id));
}

function allowedZonesForContainer(
  document: WorkbenchLayoutDocument,
  containerId: string,
): WorkbenchZoneId[] {
  const paneIds = document.containers[containerId]?.paneIds ?? [];
  return (['primarySidebar', 'editor', 'secondarySidebar', 'panel'] as WorkbenchZoneId[]).filter(
    (zoneId) => paneIds.every((paneId) => WORKBENCH_PANE_DEFINITIONS[paneId]?.allowedZones.includes(zoneId)),
  );
}

function zoneStyle(
  zoneId: WorkbenchZoneId,
  collapsed: boolean,
  sizePx: number | null,
): CSSProperties {
  if (zoneId === 'editor') return {};
  if (zoneId === 'panel') return {};
  if (collapsed) return { width: 44 };
  return { width: sizePx ?? undefined };
}

function zoneLabel(zoneId: WorkbenchZoneId): string {
  if (zoneId === 'primarySidebar') return '左侧栏';
  if (zoneId === 'secondarySidebar') return '右侧栏';
  if (zoneId === 'panel') return '底部面板';
  return '画布';
}

function zoneShortLabel(zoneId: Exclude<WorkbenchZoneId, 'editor'>): string {
  if (zoneId === 'primarySidebar') return 'Chat';
  if (zoneId === 'secondarySidebar') return '查看';
  return '运行与输出';
}

function zoneCollapseGlyph(zoneId: Exclude<WorkbenchZoneId, 'editor'>): string {
  if (zoneId === 'primarySidebar') return '‹';
  if (zoneId === 'secondarySidebar') return '›';
  return '⌄';
}

function isInteractiveTarget(target: EventTarget): boolean {
  return typeof HTMLElement !== 'undefined' && target instanceof HTMLElement &&
    Boolean(target.closest('button, select, input, textarea, a, [role="button"]'));
}

function compactWorkbench(): boolean {
  return typeof globalThis.matchMedia === 'function' && globalThis.matchMedia('(max-width: 959px)').matches;
}

function useCompactWorkbench(): boolean {
  const [compact, setCompact] = useState(compactWorkbench);
  useEffect(() => {
    if (typeof globalThis.matchMedia !== 'function') return;
    const media = globalThis.matchMedia('(max-width: 959px)');
    const update = () => setCompact(media.matches);
    update();
    media.addEventListener?.('change', update);
    return () => media.removeEventListener?.('change', update);
  }, []);
  return compact;
}
