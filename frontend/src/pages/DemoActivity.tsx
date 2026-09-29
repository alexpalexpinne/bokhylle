import { useCallback, useEffect, useState } from 'react'
import { Link } from 'react-router-dom'
import { ArrowRight, Check, Loader2 } from 'lucide-react'
import { type DemoActivity as Activity, fetchDemoActivity } from '../api/demo'
import { coverUrl } from '../api/library'
import { BookCover } from '../components/BookCover'
import { Button, ButtonLink } from '../components/ui/Button'
import { DemoSendDialog } from '../components/DemoSendDialog'
import { EmptyState } from '../components/ui/EmptyState'
import { PageHeader } from '../components/ui/PageHeader'
import { SectionMark } from '../components/ui/SectionMark'

const stages = [
  { status: 'LOOKING', label: 'Looking' },
  { status: 'FOUND', label: 'Found sample EPUB' },
  { status: 'GETTING', label: 'Adding to your shelf' },
  { status: 'READY', label: 'Ready' },
] as const

const sendStages = [
  { status: 'PREPARING', label: 'Preparing EPUB' },
  { status: 'SENDING', label: 'Sending to Demo Kindle' },
  { status: 'DELIVERED', label: 'Delivered' },
] as const

export function DemoActivity() {
  const [activity, setActivity] = useState<Activity | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [sendBook, setSendBook] = useState<{ id: number; title: string } | null>(null)
  const refresh = useCallback(async () => {
    try {
      setActivity(await fetchDemoActivity())
      setError(null)
    } catch {
      setError('Could not load demo activity. Try refreshing the page.')
    }
  }, [])

  useEffect(() => {
    void fetchDemoActivity()
      .then((result) => { setActivity(result); setError(null) })
      .catch(() => setError('Could not load demo activity. Try refreshing the page.'))
  }, [])
  const active = (activity?.gets.some((item) => item.status !== 'READY') ?? false)
    || (activity?.sends.some((item) => item.status !== 'DELIVERED') ?? false)
  useEffect(() => {
    if (!active) return
    const timer = window.setInterval(() => { void refresh() }, 2000)
    return () => window.clearInterval(timer)
  }, [active, refresh])

  return (
    <section>
      <PageHeader
        eyebrow="Demo activity"
        title="From shelf to Kindle"
        description="Follow your books onto your shelf and try sending them to Demo Kindle. Progress is simulated using prepared sample EPUBs."
        actions={<ButtonLink to="/discover" variant="secondary">Discover books</ButtonLink>}
      />
      {error && <p role="alert" className="mt-7 border-l-2 border-danger pl-4 text-sm text-danger">{error}</p>}
      {!activity && !error && <p role="status" className="mt-8 text-sm text-ink-muted">Loading activity…</p>}
      {activity && activity.gets.length === 0 && activity.sends.length === 0 && (
        <EmptyState className="mt-10" title="Nothing in motion yet" message="Choose a sample book in Discover and select Try Get to follow it here." />
      )}
      {activity && activity.gets.length > 0 && (
        <div className="mt-10">
          <SectionMark number="01" title="Getting books" />
          <ol className="mt-5 divide-y divide-line">
            {activity.gets.map((item) => {
              const position = stages.findIndex((stage) => stage.status === item.status)
              return (
                <li key={item.id} className="flex gap-5 py-5 sm:gap-7">
                  <BookCover src={coverUrl(item.bookId)} className="h-28 w-[75px] shrink-0 rounded-[3px] shadow-card" />
                  <div className="min-w-0 flex-1">
                    <p className="font-display text-xl text-ink">{item.title}</p>
                    <p className="mt-1 text-xs text-ink-muted">{item.status === 'READY' ? 'A prepared EPUB is now on your shelf.' : 'Demo acquisition in progress'}</p>
                    {item.sendWhenReady && <p className="mt-2 text-sm text-ink-soft">{item.status === 'READY' ? 'Kindle delivery is shown below.' : 'It will be sent to Demo Kindle as soon as it is on your shelf.'}</p>}
                    <ol className="mt-4 flex flex-wrap gap-x-4 gap-y-2">
                      {stages.map((stage, index) => (
                        <li key={stage.status} className={`flex items-center gap-1.5 text-xs ${index <= position ? 'text-ink' : 'text-ink-faint'}`}>
                          {index < position || item.status === 'READY' ? <Check size={13} aria-hidden /> : index === position ? <Loader2 size={13} className="animate-spin" aria-hidden /> : <span className="inline-block h-[13px] w-[13px] rounded-full border border-current" aria-hidden />}
                          {stage.label}
                        </li>
                      ))}
                    </ol>
                    {item.status === 'READY' && (
                      <div className="mt-4 flex flex-wrap items-center gap-4">
                        <Link to={`/library/${item.bookId}`} className="inline-flex items-center gap-1 text-sm font-medium text-accent hover:text-accent-strong">
                          Open book <ArrowRight size={14} aria-hidden />
                        </Link>
                        {!item.sendWhenReady && <Button variant="secondary" size="sm" onClick={() => setSendBook({ id: item.bookId, title: item.title })}>Send to Demo Kindle</Button>}
                      </div>
                    )}
                  </div>
                </li>
              )
            })}
          </ol>
        </div>
      )}
      {activity && activity.sends.length > 0 && (
        <div className="mt-10">
          <SectionMark number="02" title="Reader delivery" />
          <p className="mt-4 text-sm text-ink-muted">These actions are demonstrations. No email or file was sent to a device.</p>
          <ol className="mt-4 divide-y divide-line">
            {activity.sends.map((item) => {
              const position = sendStages.findIndex((stage) => stage.status === item.status)
              return (
                <li key={item.id} className="flex gap-5 py-5 sm:gap-7">
                  <BookCover src={coverUrl(item.bookId)} className="h-28 w-[75px] shrink-0 rounded-[3px] shadow-card" />
                  <div className="min-w-0 flex-1">
                    <p className="font-display text-xl text-ink">{item.title}</p>
                    <p className="mt-1 text-xs text-ink-muted">EPUB → Demo Kindle · {new Date(item.createdAt * 1000).toLocaleTimeString()}</p>
                    <ol aria-label={`Delivery progress for ${item.title}`} className="mt-4 flex flex-wrap gap-x-4 gap-y-2">
                      {sendStages.map((stage, index) => (
                        <li key={stage.status} aria-current={index === position ? 'step' : undefined} className={`flex items-center gap-1.5 text-xs ${index <= position ? 'text-ink' : 'text-ink-faint'}`}>
                          {index < position || item.status === 'DELIVERED' ? <Check size={13} aria-hidden /> : index === position ? <Loader2 size={13} className="animate-spin" aria-hidden /> : <span className="inline-block h-[13px] w-[13px] rounded-full border border-current" aria-hidden />}
                          {stage.label}
                        </li>
                      ))}
                    </ol>
                    <p role="status" className="mt-3 text-sm text-ink-soft">{item.status === 'DELIVERED' ? 'Delivered to Demo Kindle — simulation complete.' : item.status === 'SENDING' ? 'Your book is on its way to Demo Kindle…' : 'Preparing your book for Demo Kindle…'}</p>
                  </div>
                </li>
              )
            })}
          </ol>
        </div>
      )}
      {sendBook && <DemoSendDialog bookId={sendBook.id} title={sendBook.title} onClose={() => { setSendBook(null); void refresh() }} />}
    </section>
  )
}
