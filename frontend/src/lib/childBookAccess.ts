export const CHILD_BOOK_ACCESS = [
  {
    value: 'assigned',
    label: 'Assigned books only',
    description: 'They can only see books you put on their shelf.',
    canDiscover: false,
    canRequest: false,
  },
  {
    value: 'search',
    label: 'Search and ask',
    description: 'They can search for a book by title, author or ISBN and ask an adult for it.',
    canDiscover: false,
    canRequest: true,
  },
  {
    value: 'explore',
    label: 'Explore and ask',
    description: 'They can explore Discover, suggestions and book details, and ask an adult for books they find.',
    canDiscover: true,
    canRequest: true,
  },
] as const

export function childBookAccess(permissions: { canDiscover?: boolean; canRequest?: boolean }) {
  if (permissions.canDiscover) return permissions.canRequest ? 'explore' : 'browse-only'
  return permissions.canRequest ? 'search' : 'assigned'
}
