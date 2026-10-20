// Shows why the 3D view failed (usually WebGL 2 unavailable) instead of a blank panel.

import { Component, type ReactNode } from 'react';

interface State {
  error: Error | null;
}

export class CanvasErrorBoundary extends Component<{ children: ReactNode }, State> {
  state: State = { error: null };

  static getDerivedStateFromError(error: Error): State {
    return { error };
  }

  componentDidCatch(error: Error): void {
    console.error('[viewport] crashed', error);
  }

  render(): ReactNode {
    if (this.state.error) {
      return (
        <div className="viewport-error" role="alert">
          <strong>The 3D view could not start.</strong>
          <span>{this.state.error.message}</span>
        </div>
      );
    }
    return this.props.children;
  }
}
