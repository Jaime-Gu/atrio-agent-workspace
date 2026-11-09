import React, { Component } from "react";
import type { ReactNode } from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import "./styles.css";

interface AppErrorBoundaryState {
  failed: boolean;
}

/**
 * Last-resort boundary for render failures outside a module card (sidebar,
 * canvas header, dialogs, approval diff). Without it a single bad render
 * unmounts the whole app and leaves a blank screen.
 */
class AppErrorBoundary extends Component<
  { children: ReactNode },
  AppErrorBoundaryState
> {
  state: AppErrorBoundaryState = { failed: false };

  static getDerivedStateFromError(): AppErrorBoundaryState {
    return { failed: true };
  }

  componentDidCatch(error: unknown) {
    console.error("Atrio 界面遇到未处理的错误", error);
  }

  render() {
    if (!this.state.failed) return this.props.children;
    return (
      <main className="loading-screen" role="alert">
        <div className="brand-glyph">▦</div>
        <h1>界面遇到问题</h1>
        <p>应用出现意外错误。你可以重试，或重新加载页面。</p>
        <button
          className="button"
          onClick={() => this.setState({ failed: false })}
        >
          重试
        </button>
        <button className="button" onClick={() => location.reload()}>
          重新加载
        </button>
      </main>
    );
  }
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <AppErrorBoundary>
      <App />
    </AppErrorBoundary>
  </React.StrictMode>,
);
