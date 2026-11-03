import React from 'react'
import { render, screen } from '@testing-library/react'
import { MemoryRouter, Route, Routes } from 'react-router-dom'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const device = vi.hoisted(() => ({ isMobile: false, isTablet: false }))

vi.mock('../../hooks/useDevice', () => ({
  useDevice: () => ({ ...device, isDesktop: !device.isMobile && !device.isTablet }),
}))

import DefaultLanding from './DefaultLanding'

function renderLanding() {
  render(
    <MemoryRouter initialEntries={['/']}>
      <Routes>
        <Route path="/" element={<DefaultLanding />} />
        <Route path="/dashboard" element={<div>桌面总览</div>} />
        <Route path="/overview" element={<div>移动工作台</div>} />
      </Routes>
    </MemoryRouter>,
  )
}

describe('DefaultLanding', () => {
  beforeEach(() => {
    device.isMobile = false
    device.isTablet = false
  })

  it('uses the system overview as the desktop authenticated landing', () => {
    renderLanding()
    expect(screen.getByText('桌面总览')).toBeTruthy()
  })

  it('keeps mobile and tablet users on the existing workbench', () => {
    device.isMobile = true
    renderLanding()
    expect(screen.getByText('移动工作台')).toBeTruthy()
  })

  it('keeps tablet users on the existing workbench', () => {
    device.isTablet = true
    renderLanding()
    expect(screen.getByText('移动工作台')).toBeTruthy()
  })
})
