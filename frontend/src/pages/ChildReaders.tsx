import { useEffect, useState } from 'react'
import { Link, useParams } from 'react-router-dom'
import { ApiError } from '../api/client'
import {
  createChildReader,
  createChildReaderToken,
  defaultChildReader,
  deleteChildReader,
  fetchChildReaderTokens,
  fetchChildReaders,
  revokeChildReaderToken,
  updateChildReader,
} from '../api/childReaders'
import type { DeliveryTarget } from '../api/delivery'
import type { ReaderToken } from '../api/profile'
import { fetchUsers } from '../api/users'
import { Button } from '../components/ui/Button'
import { Modal } from '../components/ui/Modal'
import { PageHeader } from '../components/ui/PageHeader'

type ReaderEditor = {
  target?: DeliveryTarget
  name: string
  address: string
  deviceType: 'kindle' | 'pocketbook' | 'other'
}

function errorMessage(error: unknown) {
  return error instanceof ApiError ? error.message : 'Could not finish reader setup. Try again.'
}

export function ChildReaders() {
  const { id } = useParams()
  const userId = Number(id)
  const validUserId = Number.isSafeInteger(userId) && userId > 0
  const [name, setName] = useState<string | null>(null)
  const [targets, setTargets] = useState<DeliveryTarget[]>([])
  const [tokens, setTokens] = useState<ReaderToken[]>([])
  const [loading, setLoading] = useState(validUserId)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(validUserId ? null : 'Child profile not found.')
  const [notice, setNotice] = useState<string | null>(null)
  const [editor, setEditor] = useState<ReaderEditor | null>(null)
  const [removing, setRemoving] = useState<DeliveryTarget | null>(null)
  const [tokenName, setTokenName] = useState('')
  const [freshToken, setFreshToken] = useState<string | null>(null)

  useEffect(() => {
    if (!validUserId) return
    let active = true
    Promise.all([fetchUsers(), fetchChildReaders(userId), fetchChildReaderTokens(userId)])
      .then(([users, readers, readerTokens]) => {
        if (!active) return
        const child = users.find((user) => user.id === userId)
        setName(child?.displayName ?? child?.username ?? null)
        setTargets(readers)
        setTokens(readerTokens.tokens)
      })
      .catch((caught: unknown) => { if (active) setError(errorMessage(caught)) })
      .finally(() => { if (active) setLoading(false) })
    return () => { active = false }
  }, [userId, validUserId])

  async function refreshTargets() {
    setTargets(await fetchChildReaders(userId))
  }

  async function act(action: () => Promise<unknown>, success: string): Promise<boolean> {
    setBusy(true)
    setError(null)
    setNotice(null)
    try {
      await action()
      await refreshTargets()
      setNotice(success)
      return true
    } catch (caught) {
      setError(errorMessage(caught))
      return false
    } finally {
      setBusy(false)
    }
  }

  async function saveReader() {
    if (!editor) return
    const current = editor
    const saved = await act(async () => {
      if (current.target) {
        await updateChildReader(userId, current.target.id, { name: current.name, address: current.address })
      } else {
        await createChildReader(userId, { name: current.name, address: current.address, connector: 'email', deviceType: current.deviceType })
      }
    }, current.target ? 'Reader updated.' : 'Reader added.')
    if (saved) setEditor(null)
  }

  async function createToken() {
    setBusy(true)
    setError(null)
    try {
      const created = await createChildReaderToken(userId, tokenName.trim() || 'Reader')
      setFreshToken(created.token)
      setTokenName('')
      setTokens((await fetchChildReaderTokens(userId)).tokens)
    } catch (caught) {
      setError(errorMessage(caught))
    } finally {
      setBusy(false)
    }
  }

  return (
    <section>
      <PageHeader
        eyebrow="Administration · Household"
        title={name ? `Readers for ${name}` : 'Child readers'}
        description="Choose where approved books go for this child."
        actions={<Link to="/settings/household" className="text-sm font-medium text-accent hover:text-accent-strong">Back to Household</Link>}
      />
      {error && !editor && !removing && <p role="alert" className="mt-6 border-l-2 border-danger pl-4 text-sm text-danger">{error}</p>}
      {notice && <p role="status" className="mt-6 border-l-2 border-success pl-4 text-sm text-ink-soft">{notice}</p>}

      <section className="mt-8 rounded-panel bg-surface p-5 sm:p-6" aria-labelledby="child-email-reader-heading">
        <div className="flex flex-wrap items-start justify-between gap-3">
          <div>
            <h2 id="child-email-reader-heading" className="font-display text-title text-ink">Email readers</h2>
            <p className="mt-2 text-sm text-ink-muted">Kindle, PocketBook, or another reader with an email address. SMTP must be configured in <Link to="/settings/delivery" className="text-accent hover:text-accent-strong">Delivery</Link>.</p>
          </div>
          <Button variant="primary" size="sm" disabled={loading} onClick={() => { setError(null); setEditor({ name: 'Kindle', address: '', deviceType: 'kindle' }) }}>Add reader</Button>
        </div>
        {loading ? <p role="status" className="mt-6 text-sm text-ink-muted">Loading readers…</p> : targets.length === 0 ? (
          <p className="mt-6 text-sm text-ink-muted">No reader is set up for this child. Approved requests use the household fallback if one is configured in Delivery.</p>
        ) : (
          <ul className="mt-6 divide-y divide-line">
            {targets.map((target) => (
              <li key={target.id} className="flex flex-wrap items-center gap-3 py-4">
                <div className="min-w-0 flex-1">
                  <p className="text-sm font-medium text-ink">{target.name}{target.isDefault && <span className="ml-2 text-xs font-normal text-accent">Default</span>}{!target.enabled && <span className="ml-2 text-xs font-normal text-ink-faint">Disabled</span>}</p>
                  <p className="mt-1 truncate text-xs text-ink-muted">{target.address}</p>
                </div>
                <div className="flex flex-wrap gap-1">
                  {target.enabled && !target.isDefault && <Button variant="ghost" size="sm" disabled={busy} onClick={() => void act(() => defaultChildReader(userId, target.id), 'Default reader changed.')}>Set default</Button>}
                  <Button variant="ghost" size="sm" disabled={busy} onClick={() => { setError(null); setEditor({ target, name: target.name, address: target.address, deviceType: target.type as ReaderEditor['deviceType'] }) }}>Edit</Button>
                  <Button variant="ghost" size="sm" disabled={busy} onClick={() => void act(() => updateChildReader(userId, target.id, { enabled: !target.enabled }), target.enabled ? 'Reader disabled.' : 'Reader enabled.')}>{target.enabled ? 'Disable' : 'Enable'}</Button>
                  <Button variant="danger" size="sm" disabled={busy} onClick={() => { setError(null); setRemoving(target) }}>Remove</Button>
                </div>
              </li>
            ))}
          </ul>
        )}
      </section>

      <section className="mt-5 rounded-panel bg-surface p-5 sm:p-6" aria-labelledby="child-app-reader-heading">
        <h2 id="child-app-reader-heading" className="font-display text-title text-ink">Reader apps</h2>
        <p className="mt-2 text-sm text-ink-muted">For OPDS or KOReader, create a token for this child. The app will see only books on this child’s shelf.</p>
        <p className="mt-2 text-xs text-ink-faint">OPDS: {window.location.origin}/opds · KOReader sync server: {window.location.origin}. Use any username and the token as the password. Keep the same username on each KOReader device.</p>
        {tokens.length > 0 && <ul className="mt-5 divide-y divide-line">{tokens.map((token) => (
          <li key={token.id} className="flex items-center justify-between gap-3 py-3 text-sm text-ink-soft">
            <span>{token.name}</span>
            <Button variant="danger" size="sm" disabled={busy} onClick={() => void (async () => {
              setBusy(true)
              setError(null)
              try {
                await revokeChildReaderToken(userId, token.id)
                setTokens((current) => current.filter((item) => item.id !== token.id))
                setFreshToken(null)
              } catch (caught) { setError(errorMessage(caught)) }
              finally { setBusy(false) }
            })()}>Revoke</Button>
          </li>
        ))}</ul>}
        {freshToken && <p role="status" className="mt-5 border-l-2 border-accent pl-4 text-sm text-ink-soft">Copy this token now. It appears only once: <code className="break-all text-accent">{freshToken}</code></p>}
        <div className="mt-5 flex flex-wrap items-center gap-2">
          <input aria-label="Reader app name" value={tokenName} onChange={(event) => setTokenName(event.target.value)} placeholder="Reader name (e.g. Kobo)" className="min-h-10 w-full max-w-xs rounded-card bg-surface-2 px-3.5 py-2 text-sm text-ink outline-none focus-visible:outline-2 focus-visible:outline-focus" />
          <Button variant="secondary" size="sm" disabled={busy || loading} onClick={() => void createToken()}>Create token</Button>
        </div>
      </section>

      {editor && <Modal title={editor.target ? 'Edit reader' : 'Add reader'} onClose={() => setEditor(null)} footer={
        <><Button variant="ghost" onClick={() => setEditor(null)}>Cancel</Button><Button variant="primary" disabled={busy || !editor.address.trim()} onClick={() => void saveReader()}>{busy ? 'Saving…' : 'Save reader'}</Button></>
      }>
        {error && <p role="alert" className="mb-4 text-sm text-danger">{error}</p>}
        {!editor.target && <label className="block text-sm text-ink-soft">Reader type<select value={editor.deviceType} onChange={(event) => {
          const deviceType = event.target.value as ReaderEditor['deviceType']
          setEditor((current) => current && { ...current, deviceType, name: deviceType === 'pocketbook' ? 'PocketBook' : deviceType === 'kindle' ? 'Kindle' : 'Reader' })
        }} className="mt-2 w-full rounded-card bg-surface-2 px-3 py-2 text-ink"><option value="kindle">Kindle</option><option value="pocketbook">PocketBook</option><option value="other">Other</option></select></label>}
        <label className="mt-5 block text-sm text-ink-soft">Reader name<input value={editor.name} onChange={(event) => setEditor((current) => current && { ...current, name: event.target.value })} className="mt-2 w-full rounded-card bg-surface-2 px-3 py-2 text-ink outline-none focus-visible:outline-2 focus-visible:outline-focus" /></label>
        <label className="mt-5 block text-sm text-ink-soft">Reader email address<input type="email" value={editor.address} onChange={(event) => setEditor((current) => current && { ...current, address: event.target.value })} className="mt-2 w-full rounded-card bg-surface-2 px-3 py-2 text-ink outline-none focus-visible:outline-2 focus-visible:outline-focus" /></label>
      </Modal>}
      {removing && <Modal title={`Remove ${removing.name}?`} onClose={() => setRemoving(null)} footer={
        <><Button variant="ghost" onClick={() => setRemoving(null)}>Cancel</Button><Button variant="danger" disabled={busy} onClick={() => void (async () => { if (await act(() => deleteChildReader(userId, removing.id), 'Reader removed.')) setRemoving(null) })()}>Remove reader</Button></>
      }><p className="text-sm text-ink-muted">Approved books will use another enabled reader or the household fallback.</p>{error && <p role="alert" className="mt-4 text-sm text-danger">{error}</p>}</Modal>}
    </section>
  )
}
