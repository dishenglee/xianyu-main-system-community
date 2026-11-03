import { fireEvent, render, screen } from '@testing-library/react'
import { expect, it, vi } from 'vitest'

import { RichComposerTextarea } from './RichComposerTextarea'

it('shows selected Xianyu emoji as the original custom image while preserving the send code', () => {
  const onChange = vi.fn()

  render(
    <RichComposerTextarea
      value="[真棒]你好"
      onChange={onChange}
      placeholder="输入消息"
    />,
  )

  expect((screen.getByRole('textbox') as HTMLTextAreaElement).value).toBe('[真棒]你好')
  expect(screen.getByAltText('[真棒]')).toBeTruthy()

  fireEvent.change(screen.getByRole('textbox'), { target: { value: '[真棒]你好呀' } })

  expect(onChange).toHaveBeenCalledWith('[真棒]你好呀')
})
