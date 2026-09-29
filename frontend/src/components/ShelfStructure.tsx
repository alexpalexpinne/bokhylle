import { useId } from 'react'

/** A tiny vector surface shared by long rails, the hero and appearance previews. */
export function ShelfSurface({ upright = false }: { upright?: boolean }) {
  const grainId = useId()
  return (
    <span className="shelf-surface" aria-hidden="true">
      <svg width="100%" height="100%" focusable="false">
        <defs>
          <pattern id={grainId} width="96" height="4" patternUnits="userSpaceOnUse" patternTransform={upright ? 'rotate(90)' : undefined}>
            <path d="M0 2h24m9 1h35m11-2h17" stroke="var(--shelf-grain)" strokeWidth="0.5" />
          </pattern>
        </defs>
        <rect width="100%" height="100%" fill="var(--shelf-body)" />
        <rect width="100%" height="100%" fill={`url(#${grainId})`} opacity="var(--shelf-grain-opacity)" />
        <rect width="100%" height="2" fill="var(--shelf-top)" />
        {upright ? (
          <>
            <rect width="1" height="100%" fill="var(--shelf-edge)" />
            <rect x="100%" width="3" height="100%" transform="translate(-3 0)" fill="var(--shelf-side)" />
          </>
        ) : (
          <>
            <rect y="100%" width="100%" height="1" transform="translate(0 -1)" fill="var(--shelf-bottom)" />
            <rect x="100%" width="2" height="100%" transform="translate(-2 0)" fill="var(--shelf-side)" />
          </>
        )}
      </svg>
    </span>
  )
}

/** Static, scalable artwork. Its position never follows the changing book cover. */
export function ShelfDecoration() {
  return (
    <svg className="shelf-decoration" viewBox="0 0 100 180" aria-hidden="true" focusable="false">
      <g className="shelf-foliage">
        <path d="M52 128C53 93 38 64 35 18M52 116c4-25 19-44 23-71M49 91C39 82 21 76 15 62" fill="none" stroke="var(--shelf-stem)" strokeWidth="1.5" />
        <path d="M36 37c-12-2-19-11-15-21 12 1 19 10 15 21Zm4 20C49 48 55 45 58 34c-12-1-21 10-18 23ZM45 75C30 73 20 63 22 53c14 0 25 9 23 22Zm5 19c11-4 17-14 14-24-12 2-17 12-14 24ZM64 79c-3-11 0-20 10-25 5 10 1 20-10 25Zm8-18c7-4 12-12 8-22-10 3-13 12-8 22ZM28 73C15 73 7 66 8 57c13-1 22 6 20 16Z" fill="var(--shelf-leaf)" />
      </g>
      <path d="M42 120h20v13c0 5 16 12 17 24 1 12-5 23-27 23s-28-11-27-23c1-12 17-19 17-24Z" fill="var(--shelf-vase)" />
      <path d="M42 120h20M36 148c-6 10-5 20 2 26" fill="none" stroke="var(--shelf-vase-edge)" strokeWidth="2" />
    </svg>
  )
}
