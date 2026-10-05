import { Suspense, lazy } from 'react'
import { Navigate, Route, Routes, useLocation, useNavigationType, useParams, type Location } from 'react-router-dom'
import { RequireAdmin, RequireAdult, RequireAuth, RequireDiscover } from './auth/RequireAuth'
import { useAuth } from './auth/useAuth'
import { BrandMark } from './components/BrandMark'
import { Layout } from './components/Layout'
import { AuthorDetailRoute } from './pages/AuthorDetail'
import { BookDetailRoute } from './pages/BookDetail'
import { ComicSeries } from './pages/ComicSeries'
import { Home } from './pages/Home'
import { Help } from './pages/Help'
import { Library } from './pages/Library'
import { Login } from './pages/Login'
import { NotFound } from './pages/NotFound'
import { Requests } from './pages/Requests'

// Heavier pages load on first visit; Home, Library and Login stay eager so
// the common paths never wait on a chunk.
const Discover = lazy(() =>
  import('./pages/Discover').then((module) => ({ default: module.Discover })),
)
const ChildDiscover = lazy(() =>
  import('./pages/ChildDiscover').then((module) => ({ default: module.ChildDiscover })),
)
const Catalogues = lazy(() =>
  import('./pages/Catalogues').then((module) => ({ default: module.Catalogues })),
)

const Recommendations = lazy(() => import('./pages/Recommendations').then((module) => ({ default: module.Recommendations })))

function DiscoverRoute() {
  const { user } = useAuth()
  return user?.profileType === 'child' ? <ChildDiscover /> : <Discover />
}
const Downloads = lazy(() =>
  import('./pages/Downloads').then((module) => ({ default: module.Downloads })),
)
const DemoActivity = lazy(() =>
  import('./pages/DemoActivity').then((module) => ({ default: module.DemoActivity })),
)

function ActivityRoute() {
  const { demo } = useAuth()
  return demo ? <DemoActivity /> : <Downloads />
}
const Onboarding = lazy(() =>
  import('./pages/Onboarding').then((module) => ({ default: module.Onboarding })),
)
const Profile = lazy(() =>
  import('./pages/Profile').then((module) => ({ default: module.Profile })),
)
const ChildSettings = lazy(() =>
  import('./pages/ChildSettings').then((module) => ({ default: module.ChildSettings })),
)

function ProfileRoute() {
  const { user } = useAuth()
  const { section } = useParams()
  if (user?.profileType !== 'child') return <Profile />
  return section ? <Navigate to="/profile" replace /> : <ChildSettings />
}
const Settings = lazy(() =>
  import('./pages/Settings').then((module) => ({ default: module.Settings })),
)
const ChildReaders = lazy(() =>
  import('./pages/ChildReaders').then((module) => ({ default: module.ChildReaders })),
)
const ClassificationReview = lazy(() =>
  import('./pages/ClassificationReview').then((module) => ({ default: module.ClassificationReview })),
)
const ReaderPage = lazy(() =>
  import('./pages/reader/ReaderPage').then((module) => ({ default: module.ReaderPage })),
)

function RouteFallback() {
  return (
    <div
      role="status"
      aria-live="polite"
      className="flex min-h-[50vh] items-center justify-center"
    >
      <BrandMark className="h-8 w-8 animate-pulse text-ink-faint" />
      <span className="sr-only">Loading…</span>
    </div>
  )
}

export default function App() {
  const location = useLocation()
  // Routes with an explicit background location report POP internally. Keep
  // the real history action for page focus and Back/Forward restoration.
  const navigation = useNavigationType()
  const backgroundLocation = (location.state as { backgroundLocation?: Location } | null)?.backgroundLocation
  return (
    <Suspense fallback={<RouteFallback />}>
      <Routes location={backgroundLocation ?? location}>
        <Route path="/login" element={<Login />} />
        <Route element={<RequireAuth />}>
          <Route path="/welcome" element={<Onboarding />} />
          <Route path="/read/:bookId/:fileId" element={<ReaderPage />} />
        </Route>
        <Route element={<RequireAuth />}>
          <Route element={<Layout navigation={navigation} />}>
            <Route index element={<Home />} />
            <Route path="help" element={<Help />} />
            <Route element={<RequireDiscover />}>
              <Route path="discover" element={<DiscoverRoute />} />
              <Route path="recommendations" element={<Recommendations />} />
            </Route>
            <Route element={<RequireAdult />}>
              <Route path="catalogues" element={<Catalogues />} />
              <Route path="authors/:authorId" element={<AuthorDetailRoute />} />
              <Route path="activity" element={<ActivityRoute />} />
              <Route path="downloads" element={<Navigate to="/activity" replace />} />
            </Route>
            <Route path="profile" element={<ProfileRoute />} />
            <Route path="profile/:section" element={<ProfileRoute />} />
            <Route path="library" element={<Library />} />
            <Route path="requests" element={<Requests />} />
            <Route path="library/:bookId" element={<BookDetailRoute />} />
            <Route path="series/:seriesId" element={<ComicSeries />} />
            <Route element={<RequireAdmin />}>
              <Route path="library/review" element={<ClassificationReview />} />
              <Route path="settings/children/:id/readers" element={<ChildReaders />} />
              <Route path="settings" element={<Settings />} />
              <Route path="settings/:section" element={<Settings />} />
              <Route path="settings/:section/:connection" element={<Settings />} />
            </Route>
          </Route>
        </Route>
        <Route path="*" element={<NotFound />} />
      </Routes>
      {backgroundLocation && (
        <Routes>
          <Route element={<RequireAuth />}>
            <Route element={<RequireAdult />}>
              <Route path="/discover" element={<Discover detailOnly />} />
            </Route>
          </Route>
        </Routes>
      )}
    </Suspense>
  )
}
