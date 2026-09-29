import { Children, type ReactNode } from 'react'
import { ShelfSurface } from './ShelfStructure'

/** CSS selects one full-width shelf per responsive row, including partial rows. */
export function ShelfGrid({ children, className = '' }: { children: ReactNode; className?: string }) {
  return (
    <div className={`shelf-grid ${className}`}>
      {Children.map(children, (child) => (
        <div className="shelf-grid-cell">
          <ShelfSurface />
          {child}
        </div>
      ))}
    </div>
  )
}
