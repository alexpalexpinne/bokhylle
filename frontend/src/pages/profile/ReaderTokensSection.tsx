import type { Dispatch, SetStateAction } from 'react'
import type { ReaderToken } from '../../api/profile'
import { createReaderToken, fetchReaderTokens, revokeReaderToken } from '../../api/profile'
import { Button } from '../../components/ui/Button'
import { useMutation } from '../../lib/useMutation'

type ReaderTokensSectionProps = {
  tokens: ReaderToken[]
  setTokens: Dispatch<SetStateAction<ReaderToken[]>>
  name: string
  setName: (value: string) => void
  freshToken: string | null
  setFreshToken: (value: string | null) => void
  listWarning: string | null
  setListWarning: (value: string | null) => void
  tokenMutation: ReturnType<typeof useMutation>
}

export function ReaderTokensSection({
  tokens,
  setTokens,
  name,
  setName,
  freshToken,
  setFreshToken,
  listWarning,
  setListWarning,
  tokenMutation,
}: ReaderTokensSectionProps) {
  function refreshList() {
    fetchReaderTokens()
      .then((data) => setTokens(data.tokens))
      .catch((caught: unknown) => {
        console.warn('profile.reader_tokens.refresh_failed', caught)
        setListWarning('The token was created, but the list could not be refreshed — reload the page to see it.')
      })
  }

  return (
    <details className="mt-6 rounded-panel bg-surface p-5">
      <summary className="cursor-pointer list-none text-base font-semibold text-ink [&::-webkit-details-marker]:hidden">Advanced reader setup</summary>
      <p className="mt-0.5 text-xs text-ink-faint">
        Point KOReader or another OPDS reader at <span className="text-ink-soft">{window.location.origin}/opds</span> and sign in with any username and one of these tokens as the password.
      </p>
      <p className="mt-2 text-xs text-ink-faint">
        KOReader can also sync reading progress: set its sync server to <span className="text-ink-soft">{window.location.origin}</span>, pick any username (keep it the same on every device) and use the token as the password. Tokens created before progress sync existed need to be replaced; progress then feeds Home&rsquo;s Continue reading rail.
      </p>
      {tokens.length > 0 && (
        <ul className="mt-4 divide-y divide-line">
          {tokens.map((token) => (
            <li key={token.id} className="flex flex-wrap items-center justify-between gap-3 py-2.5">
              <span className="text-sm text-ink-soft">
                {token.name}
                <span className="ml-3 font-sans text-[10px] uppercase tracking-[0.14em] text-ink-faint">{token.lastUsedAt ? `last used ${new Date(token.lastUsedAt * 1000).toLocaleDateString()}` : 'never used'}</span>
              </span>
              <button
                type="button"
                disabled={tokenMutation.busyKey === `reader-${token.id}`}
                onClick={() => void tokenMutation.run(`reader-${token.id}`, () => revokeReaderToken(token.id), 'Could not revoke that token', () => setTokens((current) => current.filter((item) => item.id !== token.id)))}
                className="font-sans text-[11px] font-medium uppercase tracking-[0.16em] text-ink-muted transition-colors hover:text-danger disabled:opacity-50"
              >
                Revoke
              </button>
            </li>
          ))}
        </ul>
      )}
      {freshToken && <p className="mt-4 border-l-2 border-accent pl-3 text-sm text-ink-soft">Copy this token now — it is shown once: <code className="text-accent">{freshToken}</code></p>}
      {tokenMutation.error && <p className="mt-4 border-l-2 border-danger pl-3 text-sm text-danger">{tokenMutation.error}</p>}
      {listWarning && <p role="alert" className="mt-4 border-l-2 border-warning pl-3 text-sm text-ink-soft">{listWarning}</p>}
      <div className="mt-4 flex flex-wrap items-center gap-2">
        <input value={name} onChange={(event) => setName(event.target.value)} placeholder="Reader name (e.g. Kobo)" className="w-full max-w-xs rounded-card bg-surface-2 px-3.5 py-2 text-sm text-ink outline-none placeholder:text-ink-faint focus-visible:outline-2 focus-visible:outline-focus" />
        <Button
          variant="secondary"
          size="sm"
          disabled={tokenMutation.busyKey === 'reader-create'}
          onClick={() => void tokenMutation.run('reader-create', () => createReaderToken(name.trim() || 'Reader'), 'Could not create the token', (created) => {
            setFreshToken(created.token)
            setName('')
            setListWarning(null)
            refreshList()
          })}
        >
          Create token
        </Button>
      </div>
    </details>
  )
}
