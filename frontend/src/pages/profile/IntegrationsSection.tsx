import type { Dispatch, SetStateAction } from 'react'
import type { AgentToken } from '../../api/profile'
import { createAgentToken, fetchAgentTokens, revokeAgentToken } from '../../api/profile'
import { Button } from '../../components/ui/Button'
import { useMutation } from '../../lib/useMutation'

type IntegrationsSectionProps = {
  tokens: AgentToken[]
  setTokens: Dispatch<SetStateAction<AgentToken[]>>
  name: string
  setName: (value: string) => void
  scope: 'read' | 'write'
  setScope: (value: 'read' | 'write') => void
  freshToken: string | null
  setFreshToken: (value: string | null) => void
  listWarning: string | null
  setListWarning: (value: string | null) => void
  tokenMutation: ReturnType<typeof useMutation>
}

export function IntegrationsSection({
  tokens,
  setTokens,
  name,
  setName,
  scope,
  setScope,
  freshToken,
  setFreshToken,
  listWarning,
  setListWarning,
  tokenMutation,
}: IntegrationsSectionProps) {
  function refreshList() {
    fetchAgentTokens()
      .then((data) => setTokens(data.tokens))
      .catch((caught: unknown) => {
        console.warn('profile.agent_tokens.refresh_failed', caught)
        setListWarning('The token was created, but the list could not be refreshed — reload the page to see it.')
      })
  }

  return (
    <details open className="mt-6 rounded-panel bg-surface p-5">
      <summary className="cursor-pointer list-none text-base font-semibold text-ink [&::-webkit-details-marker]:hidden">AI &amp; integrations</summary>
      <p className="mt-0.5 text-xs text-ink-faint">
        Connect an AI assistant (ChatGPT, Claude, Cursor, …) to this profile over MCP at{' '}
        <span className="text-ink-soft">{window.location.origin}/mcp</span>. One token is one profile: its shelf, preferences and permissions apply. Read-only tokens can browse; read &amp; write tokens can also add books, ask for them and send to your reader.
      </p>
      {tokens.length > 0 && (
        <ul className="mt-4 divide-y divide-line">
          {tokens.map((token) => (
            <li key={token.id} className="flex flex-wrap items-center justify-between gap-3 py-2.5">
              <span className="text-sm text-ink-soft">
                {token.name}
                <span className="ml-3 font-sans text-[10px] uppercase tracking-[0.14em] text-ink-faint">
                  {token.scope === 'write' ? 'read & write' : 'read only'}{' · '}
                  {token.lastUsedAt ? `last used ${new Date(token.lastUsedAt * 1000).toLocaleDateString()}` : 'never used'}
                </span>
              </span>
              <button
                type="button"
                disabled={tokenMutation.busyKey === `agent-${token.id}`}
                onClick={() => void tokenMutation.run(`agent-${token.id}`, () => revokeAgentToken(token.id), 'Could not revoke that token', () => setTokens((current) => current.filter((item) => item.id !== token.id)))}
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
        <input value={name} onChange={(event) => setName(event.target.value)} placeholder="Token name (e.g. Claude on laptop)" className="w-full max-w-xs rounded-card bg-surface-2 px-3.5 py-2 text-sm text-ink outline-none placeholder:text-ink-faint focus-visible:outline-2 focus-visible:outline-focus" />
        <select value={scope} onChange={(event) => setScope(event.target.value as 'read' | 'write')} className="rounded-card bg-surface-2 px-3.5 py-2 text-sm text-ink outline-none focus-visible:outline-2 focus-visible:outline-focus">
          <option value="read">Read only</option>
          <option value="write">Read &amp; write</option>
        </select>
        <Button
          variant="secondary"
          size="sm"
          disabled={tokenMutation.busyKey === 'agent-create'}
          onClick={() => void tokenMutation.run('agent-create', () => createAgentToken(name.trim() || 'Agent', scope), 'Could not create the token', (created) => {
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
