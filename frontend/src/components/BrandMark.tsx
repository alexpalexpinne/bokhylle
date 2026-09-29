type BrandMarkProps = {
  size?: number
  className?: string
}

/** The approved B/H bookshelf mark, adapted to Paper and Ink with currentColor. */
export function BrandMark({ size = 32, className = '' }: BrandMarkProps) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 756 628"
      fill="none"
      aria-hidden
      className={className}
    >
      <g fill="currentColor">
        <rect x="0" y="0" width="84" height="628" />
        <rect x="0" y="0" width="380" height="84" />
        <rect x="0" y="272" width="756" height="84" />
        <rect x="0" y="544" width="380" height="84" />
        <path d="M336,0 L380,0 L420,40 L420,588 L380,628 L336,628 Z" />
        <rect x="672" y="0" width="84" height="628" />
        <rect x="544" y="128" width="54" height="132" rx="4" />
        <rect x="610" y="156" width="56" height="104" rx="4" />
      </g>
      <rect
        x="450"
        y="92"
        width="68"
        height="168"
        rx="6"
        fill="#D46E3D"
        transform="rotate(-10 484 260)"
      />
    </svg>
  )
}
