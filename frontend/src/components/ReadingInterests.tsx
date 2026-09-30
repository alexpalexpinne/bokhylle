import { useState } from 'react'
import { GENRES } from '../lib/genres'
import { Button } from './ui/Button'
import { Input } from './ui/Field'

const MAX_INTERESTS = 24

function interestKey(topic: string) {
  const key = topic.normalize('NFKD').replace(/\p{M}/gu, '').toLowerCase().replace(/[^\p{L}\p{N}]+/gu, ' ').trim()
  return key === 'humour' ? 'humor' : key === 'sci fi' ? 'science fiction' : key
}

export function ReadingInterests({ selected, onChange, disabled = false }: {
  selected: string[]
  onChange: (subjects: string[]) => void
  disabled?: boolean
}) {
  const [query, setQuery] = useState('')
  const trimmed = query.trim()
  const normalized = interestKey(trimmed)
  const topics = [...new Map([...selected, ...GENRES].map((topic) => [interestKey(topic), topic])).values()]
  const matches = topics.filter((topic) => interestKey(topic).includes(normalized))
  const exists = topics.some((topic) => interestKey(topic) === normalized)
  const full = selected.length >= MAX_INTERESTS
  const canAdd = normalized && !exists && new TextEncoder().encode(normalized).length <= 60

  function toggle(topic: string) {
    const key = interestKey(topic)
    onChange(selected.some((item) => interestKey(item) === key)
      ? selected.filter((item) => interestKey(item) !== key)
      : [...selected, topic])
  }

  return (
    <div className="space-y-4">
      <label className="block">
        <span className="mb-1.5 block text-xs text-ink-muted">Search reading interests</span>
        <Input value={query} disabled={disabled} maxLength={60} onChange={(event) => setQuery(event.target.value)} placeholder="Find a topic, or add your own" />
      </label>
      {selected.length > 0 && (
        <div>
          <p className="mb-2 text-xs text-ink-muted">Your interests · {selected.length} of {MAX_INTERESTS}</p>
          <div className="flex flex-wrap gap-2" role="group" aria-label="Selected reading interests">
            {selected.map((topic) => <Button key={topic} variant="secondary" size="sm" disabled={disabled} aria-label={`Remove interest ${topic}`} onClick={() => toggle(topic)}>{topic} ×</Button>)}
          </div>
        </div>
      )}
      <div className="flex flex-wrap gap-2" role="group" aria-label="Reading interest suggestions">
        {matches.map((topic) => {
          const active = selected.some((item) => interestKey(item) === interestKey(topic))
          return <Button key={topic} variant={active ? 'primary' : 'ghost'} aria-pressed={active} disabled={disabled || (full && !active)} onClick={() => toggle(topic)}>{topic}</Button>
        })}
      </div>
      {canAdd && (
        <Button variant="secondary" disabled={disabled || full} onClick={() => { onChange([...selected, trimmed]); setQuery('') }}>Add “{trimmed}”</Button>
      )}
      {full && <p role="status" className="text-xs text-ink-muted">You have chosen {MAX_INTERESTS} interests. Remove one to add another.</p>}
    </div>
  )
}
