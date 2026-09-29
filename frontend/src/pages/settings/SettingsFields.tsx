import { useCallback, useEffect, useState } from 'react'
import { useBeforeUnload, useBlocker } from 'react-router-dom'
import { ApiError } from '../../api/client'
import { type ConnectionTest, fetchAdminSettings, updateSetting } from '../../api/settings'
import { Button } from '../../components/ui/Button'
import { Modal } from '../../components/ui/Modal'
import type { Field, Group } from './groups'

type Edits = Record<string, string | boolean>

export function SettingsFields({
  groups,
  onSaved,
}: {
  groups: Group[]
  onSaved?: () => void
}) {
  const [values, setValues] = useState<Record<string, unknown>>({})
  const [envOverrides, setEnvOverrides] = useState<Record<string, string>>({})
  const [secretsConfigured, setSecretsConfigured] = useState<Record<string, boolean>>({})
  const [edits, setEdits] = useState<Edits>({})
  const [loading, setLoading] = useState(true)
  const [saving, setSaving] = useState(false)
  const [message, setMessage] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [testResult, setTestResult] = useState<Record<string, string>>({})
  const [testing, setTesting] = useState<string | null>(null)

  async function load(): Promise<boolean> {
    try {
      const data = await fetchAdminSettings()
      setValues(data.settings)
      setEnvOverrides(data.envOverrides)
      setSecretsConfigured(data.secretsConfigured)
      setEdits({})
      setError(null)
      return true
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : 'Failed to load settings')
      return false
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    void load()
  }, [])

  function valueOf(field: Field): string | boolean {
    if (field.key in edits) {
      return edits[field.key]
    }
    if (field.kind === 'boolean') {
      return Boolean(values[field.key] ?? false)
    }
    const current = values[field.key]
    if (current === undefined || current === null) {
      return ''
    }
    return String(current)
  }

  function setField(field: Field, value: string | boolean) {
    setEdits((current) => {
      const next = { ...current }
      const original = values[field.key]
      const originalValue =
        field.kind === 'boolean'
          ? Boolean(original ?? false)
          : original === undefined || original === null
            ? ''
            : String(original)

      if (value === originalValue) {
        delete next[field.key]
      } else {
        next[field.key] = value
      }
      return next
    })
  }

  async function save() {
    const changed = Object.entries(edits)
    if (changed.length === 0) {
      return
    }

    setSaving(true)
    setMessage(null)
    setError(null)

    try {
      for (const [key, value] of changed) {
        const field = groups.flatMap((group) => group.fields).find((item) => item.key === key)
        let parsed: unknown = value
        if (field?.kind === 'number') {
          parsed = Number(value)
        }
        await updateSetting(key, parsed)
      }
      const reloaded = await load()
      if (reloaded) {
        setMessage('Settings saved.')
        onSaved?.()
      } else {
        setError('Settings saved, but could not reload their current values. Reload this page before editing again.')
      }
    } catch (caught) {
      // Settings are written one key at a time: a failure can leave the
      // server partially updated, so reload the persisted values and say so.
      const reloaded = await load()
      setError(
        `${
          caught instanceof ApiError ? caught.message : 'Could not save settings'
        }. Some settings may already have been saved; ${
          reloaded
            ? 'the persisted values have been reloaded.'
            : 'reloading the persisted values failed — reload the page before editing again.'
        }`,
      )
    } finally {
      setSaving(false)
    }
  }

  async function runTest(key: string, run: () => Promise<ConnectionTest>) {
    setTesting(key)
    setTestResult((current) => ({ ...current, [key]: '' }))
    try {
      const result = await run()
      setTestResult((current) => ({
        ...current,
        [key]: 'version' in result && result.version ? `Connected: ${result.version}` : 'Connected',
      }))
    } catch (caught) {
      setTestResult((current) => ({
        ...current,
        [key]: caught instanceof ApiError ? caught.message : 'Connection failed',
      }))
    } finally {
      setTesting(null)
    }
  }

  const changedCount = Object.keys(edits).length
  const blocker = useBlocker(changedCount > 0 && !saving)

  useBeforeUnload(
    useCallback(
      (event) => {
        if (changedCount > 0 && !saving) {
          event.preventDefault()
          event.returnValue = ''
        }
      },
      [changedCount, saving],
    ),
  )

  const inputClass =
    'w-full rounded-card bg-surface-2 px-3.5 py-2.5 text-sm text-ink outline-none placeholder:text-ink-faint focus-visible:outline-2 focus-visible:outline-focus'

  return (
    <div>
      <p className="border-b border-line pb-4 text-xs text-ink-muted">
        Environment variables take precedence over values saved here.
      </p>
      {loading && <p className="mt-8 text-sm text-ink-muted">Loading settings…</p>}

      {!loading && (
        <div className="mt-6 space-y-8">
          {groups.map((group) => (
            <section key={group.title} className="border-t border-line pt-6 first:border-t-0 first:pt-0">
              <div className="flex flex-wrap items-start justify-between gap-3">
                <div>
                  <h2 className="text-base font-semibold text-ink">{group.title}</h2>
                  {group.description && (
                    <p className="mt-0.5 text-xs text-ink-faint">{group.description}</p>
                  )}
                </div>

                {group.test && (
                  <div className="flex flex-wrap items-center gap-2">
                    {testResult[group.title] && (
                      <span role="status" className="text-xs text-ink-muted">
                        {testResult[group.title]}
                      </span>
                    )}
                    <Button
                      variant="secondary"
                      size="sm"
                      onClick={() => void runTest(group.title, group.test!.run)}
                      disabled={testing === group.title || changedCount > 0}
                    >
                      {testing === group.title ? 'Testing…' : group.test.label}
                    </Button>
                  </div>
                )}
              </div>

              <div className="mt-5 grid gap-4 sm:grid-cols-2">
                {group.fields.map((field) => (
                  <label key={field.key} className="block">
                    <span className="mb-1.5 flex items-center gap-2 text-xs font-medium text-ink-muted">
                      {field.label}
                      {envOverrides[field.key] !== undefined && (
                        <span className="rounded-[3px] bg-surface-3 px-2 py-0.5 font-sans text-[10px] font-medium uppercase tracking-[0.14em] text-ink">
                          env override
                        </span>
                      )}
                      {field.kind === 'secret' && secretsConfigured[field.key] && (
                        <span className="rounded-[3px] bg-surface-3 px-2 py-0.5 font-sans text-[10px] font-medium uppercase tracking-[0.14em] text-ink">
                          configured
                        </span>
                      )}
                    </span>

                    {field.kind === 'boolean' ? (
                      <input
                        type="checkbox"
                        checked={Boolean(valueOf(field))}
                        onChange={(event) => setField(field, event.target.checked)}
                        className="h-4 w-4 accent-[var(--color-accent)]"
                      />
                    ) : field.kind === 'select' ? (
                      <select
                        value={String(valueOf(field))}
                        onChange={(event) => setField(field, event.target.value)}
                        className={inputClass}
                      >
                        <option value="">Default</option>
                        {field.options?.map((option) => (
                          <option key={option.value} value={option.value}>
                            {option.label}
                          </option>
                        ))}
                      </select>
                    ) : (
                      <input
                        type={field.kind === 'number' ? 'number' : 'text'}
                        value={String(valueOf(field))}
                        placeholder={
                          field.kind === 'secret' && secretsConfigured[field.key]
                            ? 'Configured; enter to replace'
                            : field.placeholder
                        }
                        onChange={(event) => setField(field, event.target.value)}
                        className={inputClass}
                      />
                    )}

                    {field.hint && (
                      <span className="mt-1.5 block text-xs text-ink-faint">{field.hint}</span>
                    )}
                    {envOverrides[field.key] !== undefined && (
                      <span className="mt-1.5 block text-xs text-ink-faint">
                        The environment value is active. A value saved here takes effect after the override is removed.
                      </span>
                    )}
                  </label>
                ))}
              </div>
            </section>
          ))}
        </div>
      )}
      {message && <p role="status" className="mt-6 text-sm text-ink-soft">{message}</p>}
      {error && <p role="alert" className="mt-6 text-sm text-danger">{error}</p>}
      {!loading && (
        <div className="mt-6 flex flex-wrap items-center justify-between gap-3 border-t border-line pt-5">
          <span className="text-xs text-ink-muted">
            {changedCount > 0 ? `${changedCount} unsaved change${changedCount === 1 ? '' : 's'}` : 'All changes saved'}
          </span>
          <Button variant="primary" onClick={() => void save()} disabled={saving || changedCount === 0}>
            {saving
              ? 'Saving…'
              : changedCount > 0
                ? `Save ${changedCount} change${changedCount === 1 ? '' : 's'}`
                : 'Saved'}
          </Button>
        </div>
      )}
      {blocker.state === 'blocked' && (
        <Modal title="Discard unsaved changes?" onClose={() => blocker.reset()}>
          <p className="text-sm text-ink-soft">Save this section before leaving, or discard its changes.</p>
          <div className="mt-6 flex flex-wrap justify-end gap-2">
            <Button onClick={() => blocker.reset()}>Keep editing</Button>
            <Button variant="danger" onClick={() => blocker.proceed()}>
              Discard changes
            </Button>
          </div>
        </Modal>
      )}
    </div>
  )
}
