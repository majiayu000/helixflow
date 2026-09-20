import { useEffect, useMemo, useState } from 'react';

export type CanvasPointerTool = 'pan' | 'select';

export function useCanvasPointerTools(enabled: boolean) {
  const [tool, setTool] = useState<CanvasPointerTool>('select');
  const [spaceHeld, setSpaceHeld] = useState(false);
  const [ctrlHeld, setCtrlHeld] = useState(false);

  useEffect(() => {
    if (!enabled || typeof window === 'undefined') {
      setSpaceHeld(false);
      setCtrlHeld(false);
      return;
    }
    const down = (event: KeyboardEvent) => {
      if (isEditableKeyTarget(event.target)) return;
      if (event.code === 'Space') {
        event.preventDefault();
        setSpaceHeld(true);
      }
      if (event.key === 'Control' || event.key === 'Meta') setCtrlHeld(true);
    };
    const up = (event: KeyboardEvent) => {
      if (event.code === 'Space') {
        if (!isEditableKeyTarget(event.target)) event.preventDefault();
        setSpaceHeld(false);
      }
      if (event.key === 'Control' || event.key === 'Meta') setCtrlHeld(false);
    };
    const blur = () => {
      setSpaceHeld(false);
      setCtrlHeld(false);
    };
    window.addEventListener('keydown', down);
    window.addEventListener('keyup', up);
    window.addEventListener('blur', blur);
    return () => {
      window.removeEventListener('keydown', down);
      window.removeEventListener('keyup', up);
      window.removeEventListener('blur', blur);
    };
  }, [enabled]);

  const swapped = spaceHeld || ctrlHeld;
  const activeTool = useMemo<CanvasPointerTool>(
    () => (swapped ? (tool === 'select' ? 'pan' : 'select') : tool),
    [swapped, tool],
  );

  return {
    activeTool,
    setTool,
    tool,
    panOnDrag: !enabled || activeTool === 'pan' ? true : [1, 2] as number[],
    selectionOnDrag: enabled && activeTool === 'select',
  };
}

export function isEditableKeyTarget(target: EventTarget | null): boolean {
  if (typeof Element === 'undefined' || !(target instanceof Element)) return false;
  return Boolean(target.closest('input,textarea,select,[contenteditable="true"],.composer,.wb-chat'));
}
