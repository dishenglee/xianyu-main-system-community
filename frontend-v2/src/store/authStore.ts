import { create } from 'zustand'
import { persist } from 'zustand/middleware'
import { invalidateAccountsCache } from '../hooks/accountsCache'
import { useTaskProgressStore } from './taskProgressStore'
import { useUserUnreadStore } from './userUnreadStore'

export interface AuthUser {
  username: string
  role?: string
}

interface AuthState {
  token: string
  user: AuthUser | null
  _hydrated: boolean
  setAuth: (token: string, user: AuthUser) => void
  clearAuth: () => void
}

function notifyAuthChanged() {
  if (typeof window === 'undefined') return
  window.dispatchEvent(new Event('xy:authChanged'))
}

function resetUserScopedStores() {
  useUserUnreadStore.getState().reset()
  useTaskProgressStore.getState().reset()
}

export const useAuthStore = create<AuthState>()(
  persist(
    (set) => ({
      token: 'local-single-user',
      user: { username: 'local-app', role: 'admin' },
      _hydrated: true,
      setAuth: () => {
        set({ token: 'local-single-user', user: { username: 'local-app', role: 'admin' }, _hydrated: true })
        notifyAuthChanged()
      },
      clearAuth: () => {
        try {
          localStorage.removeItem('auth_token')
        } catch {
          /* ignore */
        }
        invalidateAccountsCache()
        resetUserScopedStores()
        set({ token: 'local-single-user', user: { username: 'local-app', role: 'admin' }, _hydrated: true })
        notifyAuthChanged()
      },
    }),
    {
      name: 'auth_store',
      partialize: (state) => ({ token: state.token, user: state.user }),
      onRehydrateStorage: () => () => {
        try {
          localStorage.removeItem('auth_token')
        } catch {
          /* ignore */
        }
        useAuthStore.setState({ token: 'local-single-user', user: { username: 'local-app', role: 'admin' }, _hydrated: true })
      },
    },
  ),
)
