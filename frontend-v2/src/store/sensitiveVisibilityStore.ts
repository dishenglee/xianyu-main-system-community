import { create } from 'zustand'
import { persist } from 'zustand/middleware'

export const SENSITIVE_VISIBILITY_STORAGE_KEY = 'xianyu-sensitive-visibility'

interface SensitiveVisibilityState {
  /** Each page family keeps its own privacy preference. */
  hiddenByScope: Record<string, boolean>
  setSensitiveHidden: (scope: string, hidden: boolean) => void
  toggleSensitiveVisibility: (scope: string) => void
}

/** Persistent page-scoped privacy preferences, independent from page-local UI state. */
export const useSensitiveVisibilityStore = create<SensitiveVisibilityState>()(
  persist(
    (set) => ({
      hiddenByScope: {},
      setSensitiveHidden: (scope, hidden) => set((state) => ({
        hiddenByScope: { ...state.hiddenByScope, [scope]: hidden },
      })),
      toggleSensitiveVisibility: (scope) => set((state) => ({
        hiddenByScope: {
          ...state.hiddenByScope,
          [scope]: !state.hiddenByScope[scope],
        },
      })),
    }),
    {
      name: SENSITIVE_VISIBILITY_STORAGE_KEY,
      partialize: (state) => ({ hiddenByScope: state.hiddenByScope }),
    },
  ),
)
