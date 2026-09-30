import { useCallback, useEffect, useRef, useState, type CSSProperties, type RefObject } from 'react'
import { flushSync } from 'react-dom'

type SignInStage = 'idle' | 'fading' | 'centering' | 'opening' | 'ready'

export const loginMotion = {
  fadeMs: 160,
  slideMs: 260,
  expandMs: 420,
  easing: 'cubic-bezier(0.22, 1, 0.36, 1)',
}

async function waitForAnimations(animations: Animation[]) {
  // Returning to the chooser or leaving the page can cancel an animation.
  await Promise.all(animations.map((animation) => animation.finished.catch(() => {})))
}

// This hook owns the sign-in page's motion and viewport positioning. Account
// selection and credentials stay in Login; its callbacks commit those changes
// together with the motion state before focusing inside the original tap.
export function useLoginMotion({
  formOpen, mainRef, contentRef, profilesRef, panelRef, controlsRef, usernameRef, passwordRef,
}: {
  formOpen: boolean
  mainRef: RefObject<HTMLElement | null>
  contentRef: RefObject<HTMLDivElement | null>
  profilesRef: RefObject<HTMLDivElement | null>
  panelRef: RefObject<HTMLDivElement | null>
  controlsRef: RefObject<HTMLDivElement | null>
  usernameRef: RefObject<HTMLInputElement | null>
  passwordRef: RefObject<HTMLInputElement | null>
}) {
  const [stage, setStage] = useState<SignInStage>('idle')
  const [formTop, setFormTop] = useState<number | null>(null)
  const [pickerSize, setPickerSize] = useState({ full: 0, selected: 0 })
  const fullViewportHeightRef = useRef(window.innerHeight)
  const selectionRunRef = useRef(0)
  const selectionAnimationsRef = useRef<Animation[]>([])
  const scrollReadyRef = useRef(true)
  const scrollTargetRef = useRef<number | null>(null)

  useEffect(() => () => {
    selectionRunRef.current += 1
    selectionAnimationsRef.current.forEach((animation) => animation.cancel())
  }, [])

  const alignCredential = useCallback(() => {
    const viewport = window.visualViewport
    const active = document.activeElement
    if (!scrollReadyRef.current || !viewport || Math.abs(viewport.scale - 1) >= 0.05
      || (active !== usernameRef.current && active !== passwordRef.current)
      || !(active instanceof HTMLInputElement)) return
    const top = viewport.offsetTop + 16
    const bottom = viewport.offsetTop + viewport.height - 16
    const controls = controlsRef.current?.getBoundingClientRect()
    const field = active.closest('label')?.getBoundingClientRect() ?? active.getBoundingClientRect()
    const submit = controlsRef.current?.querySelector('button[type="submit"]')?.getBoundingClientRect()
    const remaining = submit ? { top: field.top, bottom: submit.bottom, height: submit.bottom - field.top } : field
    const profile = profilesRef.current?.querySelector('button[aria-pressed="true"]')?.getBoundingClientRect()
    const withProfile = profile && profile.height > 0 && controls
      ? { top: profile.top, bottom: controls.bottom, height: controls.bottom - profile.top } : null
    const content = contentRef.current?.getBoundingClientRect()
    const withNavigation = withProfile && content
      ? { top: withProfile.top, bottom: content.bottom, height: content.bottom - withProfile.top } : null
    // Keep the profile and switching controls visible when they fit; on a
    // shorter screen, prioritize the active field and the submit button.
    const target = [withNavigation, withProfile, controls, remaining]
      .find((bounds) => bounds != null && bounds.height <= bottom - top)
      ?? active.getBoundingClientRect()
    const distance = target.bottom > bottom ? target.bottom - bottom
      : target.top < top ? target.top - top : 0
    if (Math.abs(distance) <= 1) return
    const scrollTop = window.scrollY + distance
    // Viewport scroll events also fire during our own smooth scroll. Do not
    // restart that motion if the destination is unchanged.
    if (scrollTargetRef.current !== null && Math.abs(scrollTop - scrollTargetRef.current) <= 1) return
    scrollTargetRef.current = scrollTop
    window.scrollTo({ top: scrollTop, behavior: window.matchMedia('(prefers-reduced-motion: reduce)').matches ? 'instant' : 'smooth' })
  }, [usernameRef, passwordRef, controlsRef, profilesRef, contentRef])

  useEffect(() => {
    const viewport = window.visualViewport
    const main = mainRef.current
    if (!formOpen || !viewport || !main) return
    let frame = 0
    let settled = 0
    const update = () => {
      cancelAnimationFrame(frame)
      window.clearTimeout(settled)
      frame = requestAnimationFrame(() => {
        const active = document.activeElement
        const focused = active === usernameRef.current || active === passwordRef.current
        if (!focused) fullViewportHeightRef.current = window.innerHeight
        const keyboardOpen = focused && Math.abs(viewport.scale - 1) < 0.05
          && fullViewportHeightRef.current - viewport.height > 100
        // Some browsers shrink only the visual viewport. Add enough document
        // space to scroll the input and submit button above that occlusion.
        const layoutHeight = Math.max(window.innerHeight, document.documentElement.clientHeight)
        const inset = keyboardOpen ? Math.max(0, layoutHeight - viewport.height) : 0
        main.style.setProperty('--sign-in-keyboard-inset', `${inset}px`)
        if (!focused) scrollTargetRef.current = null
        // Let keyboard resizing settle before moving the page. Selection has
        // its own ordered animation and enables scrolling once it finishes.
        settled = window.setTimeout(alignCredential, 100)
      })
    }
    viewport.addEventListener('resize', update)
    viewport.addEventListener('scroll', update)
    main.addEventListener('focusin', update)
    main.addEventListener('focusout', update)
    const observer = new ResizeObserver(update)
    if (controlsRef.current) observer.observe(controlsRef.current)
    update()
    return () => {
      cancelAnimationFrame(frame)
      window.clearTimeout(settled)
      viewport.removeEventListener('resize', update)
      viewport.removeEventListener('scroll', update)
      main.removeEventListener('focusin', update)
      main.removeEventListener('focusout', update)
      observer.disconnect()
      main.style.removeProperty('--sign-in-keyboard-inset')
    }
  }, [formOpen, mainRef, usernameRef, passwordRef, controlsRef, alignCredential])

  function cancelSelection() {
    selectionRunRef.current += 1
    selectionAnimationsRef.current.forEach((animation) => animation.cancel())
    selectionAnimationsRef.current = []
    scrollReadyRef.current = false
    scrollTargetRef.current = null
    return selectionRunRef.current
  }

  function captureFormTop() {
    const content = contentRef.current?.getBoundingClientRect()
    const main = mainRef.current?.getBoundingClientRect()
    return content && main ? content.top - main.top : null
  }

  async function finishOpening(run: number) {
    await waitForAnimations(panelRef.current?.getAnimations() ?? [])
    if (run !== selectionRunRef.current) return
    setStage('ready')
    scrollReadyRef.current = true
    alignCredential()
  }

  async function animateSelection(button: HTMLButtonElement, run: number) {
    const picker = profilesRef.current
    if (!picker) return
    const others = [...picker.querySelectorAll('button')].filter((other) => other !== button)
    const fades = others.map((other) => other.animate(
      [{ opacity: 1 }, { opacity: 0 }],
      { duration: loginMotion.fadeMs, easing: 'ease-out', fill: 'forwards' },
    ))
    selectionAnimationsRef.current = fades
    await waitForAnimations(fades)
    if (run !== selectionRunRef.current) return
    const before = button.getBoundingClientRect()
    flushSync(() => setStage('centering'))
    const after = button.getBoundingClientRect()
    const slide = button.animate(
      [{ transform: `translate(${before.x - after.x}px, ${before.y - after.y}px)` }, { transform: 'translate(0, 0)' }],
      { duration: loginMotion.slideMs, easing: loginMotion.easing },
    )
    fades.forEach((animation) => animation.cancel())
    selectionAnimationsRef.current = [slide]
    await waitForAnimations([slide, ...picker.getAnimations()])
    if (run !== selectionRunRef.current) return
    selectionAnimationsRef.current = []
    flushSync(() => setStage('opening'))
    await finishOpening(run)
  }

  function selectProfile(button: HTMLButtonElement, updateForm: () => void) {
    const run = cancelSelection()
    const reducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches
    const top = captureFormTop()
    const full = profilesRef.current?.getBoundingClientRect().height ?? 0
    const height = button.getBoundingClientRect().height
    // Commit before focusing, while still inside the tap event. Waiting for
    // the animation would lose the user gesture needed by mobile keyboards.
    flushSync(() => {
      updateForm()
      setFormTop(top)
      setPickerSize({ full, selected: height })
      setStage(reducedMotion ? 'ready' : 'fading')
    })
    passwordRef.current?.focus({ preventScroll: true })
    if (reducedMotion) {
      scrollReadyRef.current = true
      alignCredential()
    } else {
      void animateSelection(button, run)
    }
  }

  function showUsernameForm(updateForm: () => void) {
    const run = cancelSelection()
    const top = formOpen ? formTop : captureFormTop()
    flushSync(() => {
      updateForm()
      setFormTop(top)
      setStage('opening')
    })
    usernameRef.current?.focus({ preventScroll: true })
    void finishOpening(run)
  }

  function close(updateForm: () => void) {
    cancelSelection()
    flushSync(() => {
      updateForm()
      setStage('idle')
      setFormTop(null)
    })
    scrollReadyRef.current = true
  }

  const layoutStyle: CSSProperties = {
    justifyContent: formOpen ? 'flex-start' : undefined,
    paddingTop: formOpen && formTop !== null ? formTop : undefined,
    paddingBottom: 'calc(3rem + var(--sign-in-keyboard-inset, 0px))',
  }

  return {
    stage,
    transitioning: stage !== 'idle' && stage !== 'ready',
    profileHeight: stage === 'fading' ? pickerSize.full : pickerSize.selected,
    layoutStyle,
    selectProfile,
    showUsernameForm,
    close,
    alignCredential,
  }
}
