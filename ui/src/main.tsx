import React from 'react'
import ReactDOM from 'react-dom/client'
import App from './App'
import './index.css'

/// U9：渲染异常兜底（后端字段漂移、意外运行时错误 → 显示错误页而非整树白屏）。
class ErrorBoundary extends React.Component<
  { children: React.ReactNode },
  { error: Error | null }
> {
  state = { error: null as Error | null }

  static getDerivedStateFromError(error: Error) {
    return { error }
  }

  render() {
    if (this.state.error) {
      return (
        <div className="app">
          <main className="timeline">
            <div className="error">
              <h3>⚠ 渲染异常</h3>
              <pre>{String(this.state.error)}</pre>
              <button
                className="reload"
                onClick={() => {
                  this.setState({ error: null })
                }}
              >
                重试
              </button>
            </div>
          </main>
        </div>
      )
    }
    return this.props.children
  }
}

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <ErrorBoundary>
      <App />
    </ErrorBoundary>
  </React.StrictMode>,
)
