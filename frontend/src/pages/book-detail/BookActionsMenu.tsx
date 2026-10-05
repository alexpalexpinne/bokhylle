import type { ReactNode } from 'react'
import { ActionMenu } from '../../components/ui/ActionMenu'

/** Occasional book actions share the desktop popover/mobile sheet controls. */
export function BookActionsMenu({ children }: { children: (close: () => void) => ReactNode }) {
  return <ActionMenu label="More" title="Book options" className="min-w-0 flex-1 sm:flex-none" triggerText={<span className="hidden sm:inline">More</span>}>
    {children}
  </ActionMenu>
}
