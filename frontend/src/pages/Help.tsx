import { Link } from 'react-router-dom'
import { useAuth } from '../auth/useAuth'
import { PageHeader } from '../components/ui/PageHeader'

export function Help() {
  const { user } = useAuth()
  const child = user?.profileType === 'child'

  const topics = child
    ? [
        { title: 'Find your books', body: 'Your Library shows books an administrator has approved for you. Approved requests are sent to your reader when a copy is ready.', to: '/library', action: 'Open my library' },
        { title: 'Read a book', body: 'Open an assigned EPUB, PDF, or CBZ and choose Read in Bokhylle. Your place is saved to your profile so you can continue on another browser.', to: '/library', action: 'Open my library' },
        { title: 'Ask for another book', body: 'If your household allows requests, ask an administrator for a book from Requests. If Discover is enabled, you can explore books there too. Both use the same approval process.', to: '/requests', action: 'Open requests' },
        { title: 'Make it yours', body: 'Open My settings from the account menu to choose Paper or Ink, change EPUB text size, and set a profile picture.', to: '/profile', action: 'Open my settings' },
      ]
    : [
        { title: 'Find and get a book', body: user?.canAcquire === false ? 'Search Discover by title, author, or ISBN. Existing shared books can go straight to your shelf. For a new book, ask an administrator to approve adding it.' : 'Search Discover by title, author, or ISBN. Open a result to check availability and add it to your shelf. You can keep it in the library or send it to a configured reader.', to: '/discover', action: 'Open Discover' },
        { title: 'Read what you own', body: 'Only you can browse your personal shelf. The household view contains books available to adult readers, including books that are not yet on your shelf.', to: '/library', action: 'Open Library' },
        { title: 'Remove a book', body: user?.role === 'admin' ? 'Open a book to remove it from your own shelf, or use Delete book to remove it and its files from the shared library for everyone.' : 'Open a book and remove it from your shelf. It stays in the shared library for other adult readers.', to: '/library', action: 'Open Library' },
        { title: 'Read or send a book', body: 'Open an EPUB, PDF, or CBZ and choose Read in Bokhylle to read in your browser. For another device, add an email destination or reader app token in Profile.', to: '/profile/readers', action: 'Set up a reader' },
        { title: 'Choose your preferences', body: 'Set your preferred languages and format in Profile. These choices guide discovery and new acquisitions.', to: '/profile/preferences', action: 'Open preferences' },
      ]

  return (
    <section>
      <PageHeader eyebrow="Help" title="Using Bokhylle" description="A short guide to the everyday tasks in your library." />
      <div className="mt-8 grid gap-4 md:grid-cols-2">
        {topics.map((topic) => (
          <article key={topic.title} className="rounded-panel bg-surface p-6">
            <h2 className="font-display text-title text-ink">{topic.title}</h2>
            <p className="mt-3 text-sm leading-relaxed text-ink-muted">{topic.body}</p>
            <Link to={topic.to} className="mt-5 inline-block text-sm font-medium text-accent hover:text-accent-strong">
              {topic.action} →
            </Link>
          </article>
        ))}
      </div>
      {user?.role === 'admin' && (
        <p className="mt-8 text-sm text-ink-muted">
          Managing the household? Configure optional connections in <Link to="/settings" className="text-accent hover:text-accent-strong">Administration</Link>.
        </p>
      )}
      <section aria-labelledby="source-license-heading" className="mt-10 border-t border-line pt-6">
        <h2 id="source-license-heading" className="font-display text-lg text-ink">Source and license</h2>
        <p className="mt-2 text-sm leading-relaxed text-ink-muted">
          © 2026 Bokhylle contributors. Bokhylle is free software under the GNU Affero General Public License v3.0 only. You may share and modify it under that license. It comes without warranty.
        </p>
        <div className="mt-3 flex flex-wrap gap-x-5 gap-y-2 text-sm">
          <a href="https://github.com/alexpalexpinne/bokhylle" className="font-medium text-accent hover:text-accent-strong">Get source code →</a>
          <a href="https://github.com/alexpalexpinne/bokhylle/blob/main/LICENSE" className="font-medium text-accent hover:text-accent-strong">Read the license →</a>
        </div>
      </section>
    </section>
  )
}
