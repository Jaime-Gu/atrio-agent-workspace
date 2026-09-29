import { Component } from "react";
import type { ReactNode } from "react";
import { RotateCcw, TriangleAlert } from "lucide-react";
import "./module-content.css";

interface ModuleErrorBoundaryProps {
  children: ReactNode;
  moduleName?: string;
  /** Change this value to retry automatically after replacing module state. */
  resetKey?: string | number;
}
interface ModuleErrorBoundaryState {
  failed: boolean;
}

/** Keeps a module rendering failure contained within its own card. */
export class ModuleErrorBoundary extends Component<
  ModuleErrorBoundaryProps,
  ModuleErrorBoundaryState
> {
  state: ModuleErrorBoundaryState = { failed: false };

  static getDerivedStateFromError(): ModuleErrorBoundaryState {
    return { failed: true };
  }

  componentDidUpdate(previousProps: ModuleErrorBoundaryProps) {
    if (this.state.failed && previousProps.resetKey !== this.props.resetKey) {
      this.setState({ failed: false });
    }
  }

  render() {
    if (!this.state.failed) return this.props.children;
    return (
      <div className="mc-module-error" role="alert">
        <span className="mc-module-error-icon">
          <TriangleAlert size={25} strokeWidth={1.6} />
        </span>
        <span className="mc-eyebrow">MODULE NEEDS A MOMENT</span>
        <h3>
          {this.props.moduleName
            ? `「${this.props.moduleName}」暂时无法显示`
            : "这个模块暂时无法显示"}
        </h3>
        <p>其他模块仍可使用。重新加载此模块，试着继续工作。</p>
        <button onClick={() => this.setState({ failed: false })}>
          <RotateCcw size={14} />
          重试模块
        </button>
      </div>
    );
  }
}
