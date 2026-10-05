import { Button } from './Button'

export function RetryNotice({ message, onRetry, busy = false, className = '' }: { message: string; onRetry: () => void; busy?: boolean; className?: string }) {
  return <div className={`flex flex-wrap items-center gap-x-4 gap-y-2 border-l-2 border-danger pl-4 ${className}`}>
    <p role="alert" className="text-sm text-danger">{message}</p>
    <Button size="sm" variant="ghost" className="min-h-11" disabled={busy} onClick={onRetry}>{busy ? 'Trying again…' : 'Try again'}</Button>
  </div>
}
