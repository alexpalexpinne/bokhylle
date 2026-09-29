import { useRef, useState, type DragEvent } from 'react'
import { ImageUp } from 'lucide-react'
import { ApiError } from '../api/client'
import { deleteProfilePicture, uploadProfilePicture } from '../api/profile'
import { useAuth } from '../auth/useAuth'
import { Button } from './ui/Button'
import { Modal } from './ui/Modal'
import { ProfileAvatar } from './ProfileAvatar'

const MAX_SIZE = 1024 * 1024
const TYPES = ['image/png', 'image/jpeg', 'image/webp']

export function ProfilePhotoDialog({ onClose }: { onClose: () => void }) {
  const { user, refresh } = useAuth()
  const input = useRef<HTMLInputElement>(null)
  const [dragging, setDragging] = useState(false)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  async function upload(file: File | undefined) {
    if (!file || busy) return
    if (!TYPES.includes(file.type)) {
      setError('Choose a PNG, JPEG, or WebP image.')
      return
    }
    if (file.size === 0 || file.size > MAX_SIZE) {
      setError('Choose an image no larger than 1 MB.')
      return
    }
    setBusy(true)
    setError(null)
    try {
      await uploadProfilePicture(file)
      await refresh()
      onClose()
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : 'Could not upload your picture.')
    } finally {
      setBusy(false)
    }
  }

  async function remove() {
    if (busy) return
    setBusy(true)
    setError(null)
    try {
      await deleteProfilePicture()
      await refresh()
      onClose()
    } catch (caught) {
      setError(caught instanceof ApiError ? caught.message : 'Could not remove your picture.')
    } finally {
      setBusy(false)
    }
  }

  function drop(event: DragEvent<HTMLDivElement>) {
    event.preventDefault()
    setDragging(false)
    void upload(event.dataTransfer.files[0])
  }

  return (
    <Modal title="Profile picture" description="Choose a picture for your account." onClose={onClose}>
      <div
        onDragOver={(event) => { event.preventDefault(); event.dataTransfer.dropEffect = 'copy'; setDragging(true) }}
        onDragLeave={(event) => {
          if (!(event.relatedTarget instanceof Node) || !event.currentTarget.contains(event.relatedTarget)) {
            setDragging(false)
          }
        }}
        onDrop={drop}
        className={`flex flex-col items-center gap-3 rounded-[3px] border-2 border-dashed px-5 py-7 text-center transition-colors ${dragging ? 'border-accent bg-surface-2' : 'border-line bg-surface-2/40'}`}
      >
        {user ? <ProfileAvatar user={user} className="h-20 w-20 text-3xl" /> : <ImageUp size={32} aria-hidden />}
        <p className="text-sm text-ink">Drag and drop a picture here</p>
        <p className="text-xs text-ink-muted">PNG, JPEG, or WebP · up to 1 MB</p>
        <input
          ref={input}
          type="file"
          accept="image/png,image/jpeg,image/webp"
          className="sr-only"
          tabIndex={-1}
          aria-label="Choose a profile picture"
          onChange={(event) => {
            void upload(event.target.files?.[0])
            event.target.value = ''
          }}
        />
        <Button disabled={busy} onClick={() => input.current?.click()}>
          {busy ? 'Saving…' : 'Choose picture'}
        </Button>
      </div>
      {error && <p role="alert" className="mt-3 text-sm text-danger">{error}</p>}
      {user && user.avatarVersion !== null && (
        <Button variant="danger" className="mt-4" disabled={busy} onClick={() => void remove()}>
          Remove picture
        </Button>
      )}
    </Modal>
  )
}
