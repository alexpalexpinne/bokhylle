import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { createBrowserRouter, RouterProvider } from 'react-router-dom'
import App from './App'
import { AuthProvider } from './auth/AuthProvider'
import { ErrorBoundary, RouteErrorPage } from './components/ErrorBoundary'
import './index.css'
import { initTheme } from './theme'

initTheme()

// An open tab can refer to a hashed route chunk removed by a new build.
// Refresh once to load the new index; a repeated failure reaches the router's
// recovery page instead of looping or showing its developer error screen.
window.addEventListener('vite:preloadError', (event) => {
  try {
    const key = 'bokhylle.asset-reload-at'
    const last = Number(sessionStorage.getItem(key) ?? 0)
    if (Date.now() - last < 60_000) return
    sessionStorage.setItem(key, String(Date.now()))
  } catch {
    return
  }
  event.preventDefault()
  window.location.reload()
})

const router = createBrowserRouter([{ path: '*', element: <App />, errorElement: <RouteErrorPage /> }])

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <ErrorBoundary>
      <AuthProvider>
        <RouterProvider router={router} />
      </AuthProvider>
    </ErrorBoundary>
  </StrictMode>,
)
