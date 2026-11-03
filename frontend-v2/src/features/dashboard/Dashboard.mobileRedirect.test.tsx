import React from 'react'
import { render, screen } from '@testing-library/react'
import { MemoryRouter, Route, Routes } from 'react-router-dom'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const device = vi.hoisted(() => ({ isMobile: false, isTablet: false }))
vi.mock('../../hooks/useDevice', () => ({ useDevice: () => ({ ...device }) }))
vi.mock('../../api/index', () => ({ getDataCenterStats: vi.fn() }))
vi.mock('../messages/useLiveTransport', () => ({ createLiveTransport: vi.fn() }))

import Dashboard from './Dashboard'

function renderRoute() {
  render(<MemoryRouter initialEntries={['/dashboard']}><Routes><Route path="/dashboard" element={<Dashboard />} /><Route path="/overview" element={<div>移动工作台</div>} /></Routes></MemoryRouter>)
}

describe('Dashboard mobile boundary', () => {
  beforeEach(() => { device.isMobile = false; device.isTablet = false })
  it('redirects direct mobile dashboard access to overview', () => { device.isMobile = true; renderRoute(); expect(screen.getByText('移动工作台')).toBeTruthy() })
  it('redirects direct tablet dashboard access to overview', () => { device.isTablet = true; renderRoute(); expect(screen.getByText('移动工作台')).toBeTruthy() })
})
