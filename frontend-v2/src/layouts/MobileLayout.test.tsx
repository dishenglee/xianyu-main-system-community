import { render, screen } from '@testing-library/react'
import { MemoryRouter, Route, Routes } from 'react-router-dom'
import { beforeEach, describe, expect, it, vi } from 'vitest'
import { MobileLayout } from './MobileLayout'
import { useUserUnreadStore } from '../store/userUnreadStore'

vi.mock('../components/TaskProgressFloat', () => ({
  TaskProgressFloat: () => null,
}))

vi.mock('../components/GlobalMessageNotify', () => ({
  GlobalMessageNotify: () => <div data-testid="global-message-notify" />,
}))

function renderLayout(initialPath = '/overview') {
  return render(
    <MemoryRouter initialEntries={[initialPath]}>
      <Routes>
        <Route element={<MobileLayout />}>
          <Route path="/overview" element={<div>overview page</div>} />
          <Route path="/messages/chat/:uid/:sessionId/:sessionType" element={<div>chat page</div>} />
        </Route>
      </Routes>
    </MemoryRouter>,
  )
}

describe('MobileLayout message listener', () => {
  beforeEach(() => {
    useUserUnreadStore.getState().reset()
  })

  it('mounts the global message listener outside the messages route', () => {
    renderLayout('/overview')

    expect(screen.getByTestId('global-message-notify')).toBeTruthy()
    expect(screen.getByText('overview page')).toBeTruthy()
  })

  it('keeps the listener mounted when the chat route hides bottom navigation', () => {
    renderLayout('/messages/chat/u1/s1/1')

    expect(screen.getByTestId('global-message-notify')).toBeTruthy()
    expect(screen.getByText('chat page')).toBeTruthy()
    expect(screen.queryByRole('link', { name: /消息/ })).toBeNull()
  })

  it('shows message tab badge from the live unread store', () => {
    useUserUnreadStore.getState().setUnread('u1', 's1', 1, 2)

    renderLayout('/overview')

    expect(screen.getByRole('link', { name: '消息 2 条未读' })).toBeTruthy()
  })
})
