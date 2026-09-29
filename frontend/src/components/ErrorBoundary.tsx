import { Component, type ErrorInfo, type ReactNode } from 'react'
import { useRouteError } from 'react-router-dom'
import { BrandMark } from './BrandMark'
import { Button } from './ui/Button'

type ErrorBoundaryState = { failed: boolean }

function isMissingAsset(error: unknown): boolean {
  return error instanceof Error && /dynamically imported module|loading chunk|importing a module script/i.test(error.message)
}

function RecoveryPage({ missingAsset = false }: { missingAsset?: boolean }) {
  return (
    <main className="flex min-h-screen items-center justify-center px-6">
      <div className="max-w-md text-center">
        <BrandMark className="mx-auto h-12 w-12 text-ink-faint" />
        <h1 className="mt-5 font-display text-title text-ink">
          {missingAsset ? 'This page needs a refresh' : 'Something went wrong'}
        </h1>
        <p className="mt-3 text-sm text-ink-soft">
          {missingAsset
            ? 'Bokhylle may have been updated while this page was open. Reload to get the latest version.'
            : 'We could not open this page. Reload and try again.'}
        </p>
        <div className="mt-6 flex flex-wrap items-center justify-center gap-3">
          <Button variant="primary" onClick={() => window.location.reload()}>Reload</Button>
          <Button variant="secondary" onClick={() => window.location.assign('/')}>Go home</Button>
        </div>
      </div>
    </main>
  )
}

export function RouteErrorPage() {
  const error = useRouteError()
  return <RecoveryPage missingAsset={isMissingAsset(error)} />
}

/// A render exception must not leave the SPA as a blank screen. No error
/// details are shown to readers; the recovery actions are reload and home.
export class ErrorBoundary extends Component<{ children: ReactNode }, ErrorBoundaryState> {
  state: ErrorBoundaryState = { failed: false }

  static getDerivedStateFromError(): ErrorBoundaryState {
    return { failed: true }
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error(error, info.componentStack)
  }

  render() {
    if (!this.state.failed) {
      return this.props.children
    }
    return <RecoveryPage />
  }
}
