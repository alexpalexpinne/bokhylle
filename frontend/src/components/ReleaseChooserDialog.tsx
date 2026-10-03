import { useState } from 'react'
import { Button } from './ui/Button'
import { Modal } from './ui/Modal'
import { ReleaseChoices } from './ReleaseChoices'

export function ReleaseChooserDialog({ acquisitionId, onClose, onSelected }: {
  acquisitionId: string
  onClose: () => void
  onSelected: () => void
}) {
  const [busy, setBusy] = useState(false)
  return <Modal title="Available versions" onClose={() => { if (!busy) onClose() }}
    footer={<Button variant="ghost" disabled={busy} onClick={onClose}>Close</Button>}>
    <ReleaseChoices key={acquisitionId} acquisitionId={acquisitionId} onSelected={onSelected} onBusyChange={setBusy} />
  </Modal>
}
