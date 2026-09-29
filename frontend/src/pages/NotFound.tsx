import { Link } from 'react-router-dom'
import { BrandMark } from '../components/BrandMark'

/// Unknown application routes get a real page instead of a blank screen.
export function NotFound() {
  return (
    <main className="flex min-h-screen items-center justify-center px-6">
      <div className="max-w-md text-center">
        <BrandMark className="mx-auto h-12 w-12 text-ink-faint" />
        <h1 className="mt-5 font-display text-title text-ink">Page not found</h1>
        <p className="mt-3 text-sm text-ink-soft">
          That address does not lead anywhere in Bokhylle.
        </p>
        <div className="mt-6 flex flex-wrap items-center justify-center gap-x-6 gap-y-2 text-xs font-medium uppercase tracking-[0.16em]">
          <Link to="/" className="text-accent transition-colors hover:text-accent-strong">
            Home
          </Link>
          <Link to="/library" className="text-accent transition-colors hover:text-accent-strong">
            Library
          </Link>
        </div>
      </div>
    </main>
  )
}
