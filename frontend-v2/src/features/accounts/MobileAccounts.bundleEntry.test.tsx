import React from 'react'
import { render, screen } from '@testing-library/react'
import { MemoryRouter } from 'react-router-dom'
import { describe, expect, it, vi } from 'vitest'
import MobileAccounts from './MobileAccounts'

vi.mock('./hooks/useAccountsPageState', () => ({
  useAccountsPageState: () => ({
    accounts: [],
    sortedAccounts: [],
    profiles: {},
    kpi: {
      online: 0,
      total: 0,
      sessionErrors: 0,
      totalEarned: 0,
      sold: 0,
      coin: 0,
      fp: 0,
    },
    accountQuota: { loading: false, available: true, limit: 5, sku: 'monthly' },
    enabledAccountCount: 0,
    isAdmin: false,
    loadAccounts: vi.fn(),
    showQr: false,
    qrUrl: '',
    qrStatusText: '',
    qrScanned: false,
    qrConflict: null,
    unbindApplying: false,
    handleAddAccount: vi.fn(),
    handleReloginAccount: vi.fn(),
    handleApplyAccountUnbind: vi.fn(),
    refreshQr: vi.fn(),
    closeQr: vi.fn(),
    refreshingAll: false,
    handleRefreshAll: vi.fn(),
    handleRefreshAccount: vi.fn(),
    handleDeleteAccount: vi.fn(),
    savingMap: {},
    handleToggle: vi.fn(),
    locationModal: null,
    openLocationModal: vi.fn(),
    closeLocationModal: vi.fn(),
  }),
  accountRuntimeStatus: () => ({ key: 'active' }),
}))

describe('MobileAccounts account data entry', () => {
  it('keeps the import/export entry visible before any account is added', () => {
    render(
      <MemoryRouter>
        <MobileAccounts />
      </MemoryRouter>,
    )

    expect(screen.getByRole('button', { name: '账号数据导入导出' })).toBeTruthy()
    expect(screen.getByText('暂无账号')).toBeTruthy()
  })
})
