import { useEffect, useId, useRef, type KeyboardEvent } from 'react';
import { Icon } from '../icons';
import {
  pendingNavigationLabel,
  type DirtyNavigationDecision,
  type PendingNavigation,
} from '../dirty-navigation';

export function DirtyNavigationDialog({
  busy,
  target,
  onDecision,
}: {
  busy: boolean;
  target: PendingNavigation | null;
  onDecision: (decision: DirtyNavigationDecision) => void;
}) {
  const dialogRef = useRef<HTMLDivElement | null>(null);
  const cancelRef = useRef<HTMLButtonElement | null>(null);
  const titleId = useId();

  useEffect(() => {
    if (!target || typeof document === 'undefined') return;
    const previouslyFocused = document.activeElement;
    cancelRef.current?.focus();
    return () => {
      if (typeof HTMLElement !== 'undefined' && previouslyFocused instanceof HTMLElement) {
        previouslyFocused.focus();
      }
    };
  }, [target]);

  if (!target) return null;
  const handleKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.key === 'Escape' && !busy) {
      event.preventDefault();
      onDecision('cancel');
      return;
    }
    if (event.key !== 'Tab') return;
    if (typeof document === 'undefined') return;
    const focusable = [...(dialogRef.current?.querySelectorAll<HTMLButtonElement>('button:not(:disabled)') ?? [])];
    if (focusable.length === 0) return;
    const activeIndex = focusable.indexOf(document.activeElement as HTMLButtonElement);
    const nextIndex = nextDialogFocusIndex(activeIndex, focusable.length, event.shiftKey);
    if (nextIndex === null) return;
    event.preventDefault();
    focusable[nextIndex]?.focus();
  };
  return (
    <div
      ref={dialogRef}
      className="confirm-layer"
      role="dialog"
      aria-modal="true"
      aria-labelledby={titleId}
      onKeyDown={handleKeyDown}
    >
      <div className="confirm">
        <div className="confirm-head" id={titleId}>
          <span className="ic"><Icon n="alert" s={14} /></span>
          处理未提交编辑
        </div>
        <div className="confirm-list">
          <div className="confirm-item">
            <span className="k">目标</span>
            <span className="v">{pendingNavigationLabel(target)}</span>
          </div>
          <div className="confirm-item">
            <span className="k">当前编辑</span>
            <span className="v">提交后继续，或明确放弃；取消将留在当前 workspace。</span>
          </div>
        </div>
        <div className="confirm-foot">
          <button ref={cancelRef} className="btn btn--quiet btn--sm" disabled={busy} onClick={() => onDecision('cancel')} type="button">
            取消
          </button>
          <button className="btn btn--ghost btn--sm" disabled={busy} onClick={() => onDecision('discard')} type="button">
            放弃并继续
          </button>
          <button className="btn btn--primary btn--sm" disabled={busy} onClick={() => onDecision('commit')} type="button">
            提交并继续
          </button>
        </div>
      </div>
    </div>
  );
}

export function nextDialogFocusIndex(
  activeIndex: number,
  count: number,
  backwards: boolean,
): number | null {
  if (count <= 0) return null;
  if (activeIndex < 0) return backwards ? count - 1 : 0;
  if (!backwards && activeIndex === count - 1) return 0;
  if (backwards && activeIndex === 0) return count - 1;
  return null;
}
