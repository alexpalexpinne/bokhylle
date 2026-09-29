import { NavLink, Outlet } from 'react-router-dom'
import { BookOpen, Compass, Download, Home as HomeIcon, Inbox } from 'lucide-react'
import { AccountMenu } from './AccountMenu'
import { BrandLockup } from './BrandLockup'
import { NotificationMenu } from './NotificationMenu'
import { useAuth } from '../auth/useAuth'
import { useNotifications } from '../lib/useNotifications'
import { shelfFinish } from '../lib/appearance'

const navigation = [
  { to: '/', label: 'Home', icon: HomeIcon, end: true },
  { to: '/discover', label: 'Discover', icon: Compass, end: false },
  { to: '/library', label: 'Library', icon: BookOpen, end: false },
  { to: '/activity', label: 'Activity', icon: Download, end: false },
  { to: '/requests', label: 'Requests', icon: Inbox, end: false },
]

export function Layout() {
  const { user, demo } = useAuth()
  const notifications = useNotifications(user?.id)
  const isChild = user?.profileType === 'child'
  // Requests is the child's ask flow; adults approve from the bell instead
  // and reach the full list through "View all requests".
  const items = navigation.filter((item) => {
    if (demo && item.to === '/requests') return false
    if (isChild && item.to === '/activity') {
      return false
    }
    if (isChild && item.to === '/discover' && !user?.canDiscover) return false
    return isChild || (item.to === '/requests' ? user?.canAcquire === false && user.role !== 'admin' : true)
  })

  return (
    <div data-shelf-finish={shelfFinish(user?.shelfFinish)} className="min-h-screen bg-canvas pb-24 sm:pb-0">
      <header className="sticky top-0 z-40 border-b border-line bg-canvas">
        <div className="mx-auto flex max-w-content items-center justify-between gap-6 px-4 py-4 sm:px-6 lg:px-8">
          <div className="flex items-center gap-10 lg:gap-16">
            <NavLink to="/" className="flex items-center" aria-label="Bokhylle home">
              <BrandLockup />
            </NavLink>

            <nav className="hidden items-center gap-7 md:flex">
              {items.map((item) => (
                <NavLink
                  key={item.to}
                  to={item.to}
                  end={item.end}
                  className={({ isActive }) =>
                    `border-b-2 px-1 pb-1 pt-0.5 font-sans text-[13px] font-medium transition-colors ${
                      isActive
                        ? 'border-accent text-ink'
                        : 'border-transparent text-ink-muted hover:text-ink'
                    }`
                  }
                >
                  {item.label}
                </NavLink>
              ))}
            </nav>
          </div>

          <div className="flex items-center gap-1">
            <NotificationMenu
              items={notifications.items}
              unread={notifications.unread}
              pendingRequestItems={notifications.pendingRequestItems}
              canDecide={user?.role === 'admin' || (demo === true && !isChild)}
              markRead={notifications.markRead}
              onDecided={(id) => {
                notifications.removePendingRequest(id)
                notifications.reload()
              }}
            />
            <AccountMenu />
          </div>
        </div>
      </header>

      {demo && <div className="border-b border-line bg-surface-2 px-4 py-2 text-center text-xs text-ink-muted">Public demo · Get and Send use sample books · No outside download or email · Changes may reset at any time</div>}

      <main className="mx-auto w-full max-w-content px-4 py-6 sm:px-6 sm:py-10 lg:px-8">
        <Outlet />
      </main>

      <nav className="fixed inset-x-0 bottom-0 z-40 border-t border-line bg-canvas md:hidden">
        <div
          className="grid"
          style={{ gridTemplateColumns: `repeat(${items.length}, minmax(0, 1fr))` }}
        >
          {items.map((item) => (
            <NavLink
              key={item.to}
              to={item.to}
              end={item.end}
              className={({ isActive }) =>
                `flex flex-col items-center gap-1 py-2.5 text-xs font-medium transition-colors ${
                  isActive ? 'text-accent' : 'text-ink-muted'
                }`
              }
            >
              <item.icon size={20} strokeWidth={1.8} />
              {item.label}
            </NavLink>
          ))}
        </div>
      </nav>
    </div>
  )
}
