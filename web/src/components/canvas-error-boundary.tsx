import { Component, type ReactNode } from 'react';

type CanvasErrorBoundaryProps = {
  children: ReactNode;
  resetKey: string;
};

type CanvasErrorBoundaryState = {
  failed: boolean;
};

export class CanvasErrorBoundary extends Component<
  CanvasErrorBoundaryProps,
  CanvasErrorBoundaryState
> {
  state: CanvasErrorBoundaryState = { failed: false };

  static getDerivedStateFromError(): CanvasErrorBoundaryState {
    return { failed: true };
  }

  componentDidUpdate(previous: CanvasErrorBoundaryProps): void {
    if (this.state.failed && previous.resetKey !== this.props.resetKey) {
      this.setState({ failed: false });
    }
  }

  render() {
    if (!this.state.failed) return this.props.children;

    return (
      <div className="canvas-error" role="alert">
        <div className="canvas-error__eyebrow">CANVAS RECOVERY</div>
        <h2>画布暂时无法显示</h2>
        <p>对话、版本和运行数据仍然安全。可以重新加载画布，或切换工作区后再回来。</p>
        <button type="button" className="btn" onClick={() => this.setState({ failed: false })}>
          重新加载画布
        </button>
      </div>
    );
  }
}
