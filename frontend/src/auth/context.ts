import type { components } from '../api/generated'
import { createContext } from 'react'

export type User = components['schemas']['UserView']

export type AuthState = {
  user: User | null
  loading: boolean
  demo: boolean | null
  enterDemo: (profile: 'adult' | 'child') => Promise<void>
  switchDemo: () => Promise<void>
  login: (username: string, password: string, remember?: boolean) => Promise<void>
  logout: () => Promise<void>
  logoutAll: () => Promise<void>
  refresh: () => Promise<void>
}

export const AuthContext = createContext<AuthState | null>(null)
