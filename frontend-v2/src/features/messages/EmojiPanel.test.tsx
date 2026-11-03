import { fireEvent, render, screen } from '@testing-library/react'
import { expect, it, vi } from 'vitest'

import { EmojiPanel } from './EmojiPanel'

it('inserts the original Xianyu bracket code instead of replacing it with a system emoji', () => {
  const onPick = vi.fn()

  render(
    <EmojiPanel
      open
      variant="desktop"
      onClose={() => {}}
      onPick={onPick}
    />,
  )

  fireEvent.click(screen.getByRole('button', { name: '[真棒]' }))

  expect(onPick).toHaveBeenCalledWith('[真棒]')
})
