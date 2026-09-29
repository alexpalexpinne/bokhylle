import { BrandMark } from './BrandMark'

/** Shared mark and wordmark for the app header and sign-in screen. */
export function BrandLockup({ large = false, stacked = false }: { large?: boolean; stacked?: boolean }) {
  return (
    <span className={`inline-flex items-center text-ink ${stacked ? 'flex-col gap-5' : large ? 'gap-3' : 'gap-2.5'}`}>
      <BrandMark size={stacked ? 96 : large ? 40 : 26} />
      <span
        aria-hidden
        className={`block shrink-0 bg-current ${large ? 'h-[29px] w-[146px]' : 'h-[19px] w-[96px]'}`}
        style={{
          mask: 'url(/brand/bokhylle-wordmark.svg) center / contain no-repeat',
          WebkitMask: 'url(/brand/bokhylle-wordmark.svg) center / contain no-repeat',
        }}
      />
      <span className="sr-only">Bokhylle</span>
    </span>
  )
}
