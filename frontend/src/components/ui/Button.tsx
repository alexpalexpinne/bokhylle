import type { ButtonHTMLAttributes, MouseEventHandler, ReactNode, Ref } from 'react'
import { Link } from 'react-router-dom'

type Variant = 'primary' | 'secondary' | 'ghost' | 'danger'
type Size = 'sm' | 'md' | 'lg'

const base =
  'pressable inline-flex items-center justify-center gap-2 font-medium transition-[background-color,color,transform] duration-150 ease-smooth disabled:cursor-not-allowed disabled:opacity-50'

const variants: Record<Variant, string> = {
  primary: 'rounded-[3px] bg-accent text-accent-ink hover:bg-accent-strong',
  secondary: 'rounded-[3px] bg-surface-2 text-ink hover:bg-surface-3',
  ghost: 'rounded-[3px] bg-transparent text-ink-soft hover:bg-surface-2 hover:text-ink',
  danger: 'rounded-[3px] bg-transparent text-danger hover:bg-surface-2',
}

const sizes: Record<Size, string> = {
  sm: 'min-h-12 px-3.5 text-sm sm:min-h-8',
  md: 'min-h-12 px-5 text-sm sm:min-h-10',
  lg: 'h-12 px-6 text-base',
}

type CommonProps = {
  variant?: Variant
  size?: Size
  className?: string
  children: ReactNode
}

type ButtonProps = CommonProps & ButtonHTMLAttributes<HTMLButtonElement> & { ref?: Ref<HTMLButtonElement> }

export function Button({
  variant = 'secondary',
  size = 'md',
  className = '',
  children,
  ...rest
}: ButtonProps) {
  return (
    <button
      type="button"
      className={`${base} ${variants[variant]} ${sizes[size]} ${className}`}
      {...rest}
    >
      {children}
    </button>
  )
}

type ButtonLinkProps = CommonProps & {
  to: string
  state?: unknown
}

export function ButtonLink({
  to,
  state,
  variant = 'secondary',
  size = 'md',
  className = '',
  children,
}: ButtonLinkProps) {
  return (
    <Link to={to} state={state} className={`${base} ${variants[variant]} ${sizes[size]} ${className}`}>
      {children}
    </Link>
  )
}

type ExternalButtonLinkProps = CommonProps & {
  href: string
  download?: boolean
  onClick?: MouseEventHandler<HTMLAnchorElement>
}

export function ButtonAnchor({
  href,
  download,
  onClick,
  variant = 'secondary',
  size = 'md',
  className = '',
  children,
}: ExternalButtonLinkProps) {
  return (
    <a
      href={href}
      download={download}
      onClick={onClick}
      className={`${base} ${variants[variant]} ${sizes[size]} ${className}`}
    >
      {children}
    </a>
  )
}
