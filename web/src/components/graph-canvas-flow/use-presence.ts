import { useCallback, useEffect, useRef, useState, type MouseEvent } from 'react';
import {
  CANVAS_PRESENCE_HEARTBEAT_MS,
  CANVAS_PRESENCE_INTERVAL_MS,
  LOCAL_CANVAS_ACTOR,
} from '../../canvas-presence';
import type { CanvasPresence } from '../../types';
import { createLatestAsyncPublisher } from '../latest-async-publisher';
import type { ViewState } from '../graph-canvas-navigation';
import type { Point } from '../graph-canvas-selection';
import type { WorkflowFlowInstance } from './types';

type PresenceInput = {
  workspaceId: string;
  instance: WorkflowFlowInstance | null;
  selectedIds: Set<string>;
  view: ViewState;
  onPresenceChange?: (presence: CanvasPresence) => void | Promise<void>;
};

export function useFlowPresence(input: PresenceInput) {
  const cursorRef = useRef<Point | null>(null);
  const inputRef = useRef(input);
  inputRef.current = input;
  const [publisher] = useState(() =>
    createLatestAsyncPublisher<CanvasPresence>(
      (presence) => inputRef.current.onPresenceChange?.(presence),
      CANVAS_PRESENCE_INTERVAL_MS,
    ),
  );

  const publish = useCallback((immediate = false) => {
    const current = inputRef.current;
    if (!current.onPresenceChange) return;
    publisher.schedule({
      actor: LOCAL_CANVAS_ACTOR,
      cursor: cursorRef.current,
      selection: { nodeIds: [...current.selectedIds], edgeIds: [] },
      viewport: { x: current.view.x, y: current.view.y, zoom: current.view.z },
    });
    if (immediate) publisher.flush();
  }, [publisher]);

  useEffect(() => {
    publish();
  }, [input.selectedIds, input.view.x, input.view.y, input.view.z, publish]);

  useEffect(() => {
    const heartbeat = setInterval(publish, CANVAS_PRESENCE_HEARTBEAT_MS);
    return () => {
      clearInterval(heartbeat);
      publisher.cancel();
    };
  }, [input.workspaceId, publish, publisher]);

  const onPaneMouseMove = useCallback((event: MouseEvent) => {
    const current = inputRef.current;
    cursorRef.current = current.instance?.screenToFlowPosition({
      x: event.clientX,
      y: event.clientY,
    }) ?? null;
    publish();
  }, [publish]);

  const onPaneMouseLeave = useCallback(() => {
    cursorRef.current = null;
    publish(true);
  }, [publish]);

  return { onPaneMouseLeave, onPaneMouseMove };
}
