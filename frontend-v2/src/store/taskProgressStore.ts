import { create } from 'zustand'

export interface TaskProgressResult {
  uid?: string
  label?: string
  ok: boolean
  msg: string
}

interface TaskProgressState {
  title: string
  indeterminate: boolean
  running: boolean
  done: number
  total: number
  results: TaskProgressResult[]
  visible: boolean
  start: (total: number, title?: string, options?: { indeterminate?: boolean }) => void
  tick: (done: number, results: TaskProgressResult[]) => void
  finish: (results: TaskProgressResult[]) => void
  dismiss: () => void
  reset: () => void
}

const initialTaskProgressState = {
  title: '任务',
  indeterminate: false,
  running: false,
  done: 0,
  total: 0,
  results: [],
  visible: false,
}

export const useTaskProgressStore = create<TaskProgressState>()((set) => ({
  ...initialTaskProgressState,
  start: (total, title = '任务', options = {}) => set({
    title,
    indeterminate: !!options.indeterminate,
    running: true,
    done: 0,
    total,
    results: [],
    visible: true,
  }),
  tick: (done, results) => set({ done, results }),
  finish: (results) => set((state) => ({ running: false, done: state.total || results.length, results })),
  dismiss: () => set(initialTaskProgressState),
  reset: () => set(initialTaskProgressState),
}))
