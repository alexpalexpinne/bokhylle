import { ShelfGrid } from './ShelfGrid'
import { ShelfBookSkeleton } from './ShelfRail'

export function ShelfGridSkeleton({ className = '' }: { className?: string }) {
  return <div aria-hidden="true"><ShelfGrid className={className}>
    {Array.from({ length: 6 }, (_, index) => <ShelfBookSkeleton key={index} />)}
  </ShelfGrid></div>
}
