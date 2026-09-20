import { act, create, type ReactTestRenderer } from 'react-test-renderer';
import { renderToStaticMarkup } from 'react-dom/server';
import { afterEach, describe, expect, it } from 'vitest';
import { resetWorkbenchLayoutStoreForTests, useWorkbenchLayoutStore } from '../store';
import type { WorkbenchLayoutStorage } from '../storage';
import { WorkbenchShell } from './workbench-shell';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

afterEach(async () => {
  await act(async () => resetWorkbenchLayoutStoreForTests(null));
});

describe('WorkbenchShell', () => {
  it('renders registered business content in stable zones', () => {
    const markup = renderToStaticMarkup(<WorkbenchShell panes={paneBindings()} />);
    expect(markup).not.toContain('data-workbench-zone="primarySidebar"');
    expect(markup).toContain('data-workbench-zone="editor"');
    expect(markup).toContain('data-workbench-zone="secondarySidebar"');
    expect(markup).toContain('data-workbench-zone="panel"');
    expect(markup).toContain('aria-label="展开右侧栏"');
    expect(markup).toContain('hidden=""');
    expect(markup).toContain('chat-content');
    expect(markup).toContain('canvas-content');
  });

  it('collapses a pane independently and persists through the layout owner', async () => {
    const values = new Map<string, string>();
    resetWorkbenchLayoutStoreForTests(memoryStorage(values));
    let renderer: ReactTestRenderer;
    await act(async () => { renderer = create(<WorkbenchShell panes={paneBindings()} />); });
    const open = renderer!.root.findByProps({ 'aria-label': '展开右侧栏' });
    await act(async () => open.props.onClick());

    const chatPane = renderer!.root.findByProps({ 'data-pane-id': 'chat' });
    const toggle = chatPane.findByProps({ className: 'workbench-pane-toggle' });
    await act(async () => toggle.props.onClick());

    expect(useWorkbenchLayoutStore.getState().document.panes.chat.collapsed).toBe(true);
    expect([...values.values()].join('')).toContain('"collapsed":true');
    await act(async () => renderer!.unmount());
  });

  it('uses a real pointer gesture to move a container into a legal zone', async () => {
    openPanel();
    const primaryZone = {
      dataset: { workbenchZone: 'primarySidebar' },
      getBoundingClientRect: () => ({ left: 0, right: 300, top: 0, bottom: 800 }),
    };
    const panelZone = {
      dataset: { workbenchZone: 'panel' },
      getBoundingClientRect: () => ({ left: 300, right: 1200, top: 600, bottom: 800 }),
    };
    let renderer: ReactTestRenderer;
    await act(async () => {
      renderer = create(<WorkbenchShell panes={paneBindings()} />, {
        createNodeMock: (element) => {
          const props = (element as { props?: { className?: unknown } }).props;
          return props?.className === 'workbench-shell'
            ? { querySelectorAll: () => [primaryZone, panelZone] }
            : {};
        },
      });
    });
    let runPane = renderer!.root.findByProps({ 'data-pane-id': 'run' });
    let header = runPane.findByProps({ title: '拖拽到工作台区域，双击折叠' });
    const currentTarget = pointerTarget();

    await act(async () => header.props.onPointerDown({
      button: 0, clientX: 600, clientY: 700, currentTarget, pointerId: 11, target: {},
    }));
    runPane = renderer!.root.findByProps({ 'data-pane-id': 'run' });
    header = runPane.findByProps({ title: '拖拽到工作台区域，双击折叠' });
    await act(async () => {
      header.props.onPointerMove({
        clientX: 120, clientY: 300, currentTarget, pointerId: 11,
      });
    });
    expect(renderer!.root.findByProps({ 'aria-label': '工作台停靠位置' })).toBeDefined();
    await act(async () => header.props.onPointerUp({ currentTarget, pointerId: 11 }));

    expect(useWorkbenchLayoutStore.getState().document.containers.runMonitor.zoneId).toBe('primarySidebar');
    await act(async () => renderer!.unmount());
  });

  it('keeps unavailable pane placement without rendering its content', () => {
    const bindings = paneBindings();
    bindings.outputs = { available: false, content: <div>missing-output</div> };
    const markup = renderToStaticMarkup(<WorkbenchShell panes={bindings} />);
    expect(markup).not.toContain('missing-output');
    expect(useWorkbenchLayoutStore.getState().document.panes.outputs.containerId).toBe('outputs');
  });

  it('supports keyboard resize and persists the committed zone size', async () => {
    const values = new Map<string, string>();
    resetWorkbenchLayoutStoreForTests(memoryStorage(values));
    let renderer: ReactTestRenderer;
    await act(async () => { renderer = create(<WorkbenchShell panes={paneBindings()} />); });
    const open = renderer!.root.findByProps({ 'aria-label': '展开右侧栏' });
    await act(async () => open.props.onClick());
    const sash = renderer!.root.findByProps({ 'aria-label': '调整右侧栏大小' });

    await act(async () => sash.props.onKeyDown({
      key: 'ArrowLeft', shiftKey: false, preventDefault: () => undefined,
    }));

    expect(useWorkbenchLayoutStore.getState().document.zones.secondarySidebar.sizePx).toBe(390);
    expect([...values.values()].join('')).toContain('"sizePx":390');
    await act(async () => renderer!.unmount());
  });

  it('can restore a hidden optional pane from the floating zone action', async () => {
    openPanel();
    let renderer: ReactTestRenderer;
    await act(async () => { renderer = create(<WorkbenchShell panes={paneBindings()} />); });
    const hide = renderer!.root.findByProps({ 'aria-label': '隐藏Outputs' });
    await act(async () => hide.props.onClick());
    expect(useWorkbenchLayoutStore.getState().document.panes.outputs.visible).toBe(false);

    const show = renderer!.root.findByProps({ 'aria-label': '显示Outputs' });
    await act(async () => show.props.onClick());
    expect(useWorkbenchLayoutStore.getState().document.panes.outputs.visible).toBe(true);
    await act(async () => renderer!.unmount());
  });

  it('omits the redundant panel title row and keeps a compact panel launcher', () => {
    const markup = renderToStaticMarkup(<WorkbenchShell panes={paneBindings()} />);
    expect(markup).not.toContain('<span>底部面板</span>');
    expect(markup).toContain('aria-label="展开底部面板"');
  });

  it('keeps the canvas in flow while the run panel lives in an overlay layer', () => {
    const markup = renderToStaticMarkup(<WorkbenchShell panes={paneBindings()} />);
    expect(markup).toContain('class="workbench-center"');
    expect(markup).toContain('workbench-panel-layer');
    expect(markup).not.toContain('workbench-drawer-layer--primary');
    expect(markup).toContain('workbench-drawer-layer--secondary');
    expect(markup).toContain('data-workbench-zone="panel"');
  });

  it('opens the default-collapsed run tray without changing the canvas pane', async () => {
    let renderer: ReactTestRenderer;
    await act(async () => { renderer = create(<WorkbenchShell panes={paneBindings()} />); });

    const open = renderer!.root.findByProps({ 'aria-label': '展开底部面板' });
    await act(async () => open.props.onClick());
    expect(renderer!.root.findByProps({ 'aria-label': '收起底部面板' })).toBeDefined();
    expect(renderer!.root.findByProps({ 'data-pane-id': 'canvas' })).toBeDefined();
    await act(async () => renderer!.unmount());
  });
});

function openPanel() {
  resetWorkbenchLayoutStoreForTests(null);
  useWorkbenchLayoutStore.getState().dispatch({ type: 'zone/toggle', zoneId: 'panel' });
}

function paneBindings() {
  return {
    chat: { available: true, content: <div>chat-content</div> },
    canvas: { available: true, content: <div>canvas-content</div> },
    artifact: { available: true, content: <div>artifact-content</div> },
    run: { available: true, content: <div>run-content</div> },
    outputs: { available: true, content: <div>outputs-content</div> },
  };
}

function memoryStorage(values: Map<string, string>): WorkbenchLayoutStorage {
  return {
    getItem: (key) => values.get(key) ?? null,
    setItem: (key, value) => { values.set(key, value); },
  };
}

function pointerTarget() {
  return {
    setPointerCapture: () => undefined,
    hasPointerCapture: () => true,
    releasePointerCapture: () => undefined,
  };
}
