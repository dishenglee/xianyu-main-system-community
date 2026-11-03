// @ts-nocheck
import React from 'react'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const { apiMocks } = vi.hoisted(() => ({
  apiMocks: {
    itemPublish: vi.fn(),
    itemImportLink: vi.fn(),
    uploadItemImage: vi.fn(),
    itemAiDescription: vi.fn(),
    itemKgraph: vi.fn(),
    publishCheck: vi.fn(),
    publishBadwords: vi.fn(),
    refreshAccount: vi.fn(),
    request: vi.fn(),
    locationByGps: vi.fn(),
    applyPostPublishExtras: vi.fn(),
  },
}))

vi.mock('../../../api/index', () => ({
  ...apiMocks,
  isSuccess: (resp: any) => Array.isArray(resp?.ret)
    ? resp.ret.some((value: string) => String(value).includes('SUCCESS'))
    : !!resp?.success,
  retMsg: (_resp: any, fallback: string) => fallback,
}))

import { PublishWizardSheet } from './PublishWizardSheet'

const accounts = [
  {
    uid: '90000000001',
    nick: '年丰巷',
    status: 'active',
    location: { province: '湖南省', city: '长沙市', area: '岳麓区', divisionId: '430104' },
  },
]

describe('PublishWizardSheet', () => {
  const notify = vi.fn()
  const onClose = vi.fn()
  const onPublished = vi.fn()

  beforeEach(() => {
    vi.clearAllMocks()
    apiMocks.itemPublish.mockResolvedValue({ ret: ['SUCCESS::调用成功'], data: {} })
    apiMocks.uploadItemImage.mockResolvedValue({ success: true, url: 'https://img.alicdn.com/test-phone.jpg' })
    apiMocks.itemAiDescription.mockResolvedValue({
      ret: ['SUCCESS::调用成功'],
      data: {
        desc: '苹果13 128G 国行蓝色，成色不错，功能正常，配件齐全。',
        title: '苹果13 128G 国行蓝色',
      },
    })
    apiMocks.itemKgraph.mockResolvedValue({
      ret: ['SUCCESS::调用成功'],
      data: {
        category: {
          propertyId: '-10000',
          channelCatId: '5001',
          catName: '手机',
        },
        cardList: [
          {
            propertyId: '-10000',
            propertyName: '分类',
            valuesList: [{ valueId: '5001', valueName: '手机', isClicked: '1' }],
          },
          {
            propertyId: '2001',
            propertyName: '成色',
            valuesList: [
              { valueId: '1', valueName: '全新' },
              { valueId: '2', valueName: '95新', isClicked: '1' },
            ],
          },
        ],
      },
    })
    apiMocks.request.mockResolvedValue({ tips: [] })
    apiMocks.locationByGps.mockResolvedValue({})
    apiMocks.applyPostPublishExtras.mockResolvedValue({ results: [] })
    apiMocks.publishCheck.mockResolvedValue({ ret: ['SUCCESS::调用成功'], data: { canPublish: true } })
    apiMocks.publishBadwords.mockResolvedValue({ ret: ['SUCCESS::调用成功'], data: { safe: true } })
    apiMocks.refreshAccount.mockResolvedValue({})
  })

  function renderSheet() {
    return render(
      <PublishWizardSheet
        open
        accounts={accounts}
        onClose={onClose}
        onPublished={onPublished}
        notify={notify}
      />,
    )
  }

  it('uses description-driven AI generation and no longer shows a title field', async () => {
    renderSheet()

    expect(screen.queryByText('商品标题')).toBeNull()
    fireEvent.change(screen.getByPlaceholderText('直接输入商品描述，发布时会自动生成标题'), {
      target: { value: '苹果13 128G 国行蓝色，成色不错。' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'AI写描述' }))

    await waitFor(() => {
      expect(apiMocks.itemAiDescription).toHaveBeenCalledWith('90000000001', expect.objectContaining({
        title: '',
        desc: '苹果13 128G 国行蓝色，成色不错。',
        imageUrls: [],
      }))
    })
    expect(
      (screen.getByPlaceholderText('直接输入商品描述，发布时会自动生成标题') as HTMLTextAreaElement).value,
    ).toBe('苹果13 128G 国行蓝色，成色不错，功能正常，配件齐全。')
  })

  it('derives the hidden publish title from description on submit', async () => {
    renderSheet()

    fireEvent.change(screen.getByPlaceholderText('直接输入商品描述，发布时会自动生成标题'), {
      target: { value: '苹果13 128G 国行蓝色，成色不错，功能正常。' },
    })
    const fileInput = document.body.querySelector('input[type="file"]')
    const file = new File(['fake image'], 'phone.jpg', { type: 'image/jpeg' })
    fireEvent.change(fileInput!, { target: { files: [file] } })
    await waitFor(() => {
      expect(apiMocks.uploadItemImage).toHaveBeenCalled()
    })
    fireEvent.click(screen.getByRole('button', { name: /下一步/ }))

    fireEvent.change(screen.getByPlaceholderText('0.00'), {
      target: { value: '2999' },
    })
    fireEvent.click(screen.getByRole('button', { name: /下一步/ }))

    fireEvent.click(screen.getByRole('button', { name: '确认发布' }))

    await waitFor(() => {
      expect(apiMocks.itemPublish).toHaveBeenCalledWith('90000000001', expect.objectContaining({
        title: '苹果13 128G 国行蓝色',
        desc: '苹果13 128G 国行蓝色，成色不错，功能正常。',
        imageUrls: ['https://img.alicdn.com/test-phone.jpg'],
        propSelections: expect.objectContaining({ '2001': '2' }),
      }))
    })
  })

  it('blocks submit when the account cannot publish', async () => {
    apiMocks.publishCheck.mockResolvedValueOnce({
      ret: ['SUCCESS::调用成功'],
      data: { canPublish: false, message: '账号暂不可发布' },
    })
    renderSheet()

    fireEvent.change(screen.getByPlaceholderText('直接输入商品描述，发布时会自动生成标题'), {
      target: { value: '苹果13 128G 国行蓝色，成色不错，功能正常。' },
    })
    const fileInput = document.body.querySelector('input[type="file"]')
    fireEvent.change(fileInput!, { target: { files: [new File(['fake image'], 'phone.jpg', { type: 'image/jpeg' })] } })
    await waitFor(() => expect(apiMocks.uploadItemImage).toHaveBeenCalled())
    fireEvent.click(screen.getByRole('button', { name: /下一步/ }))
    fireEvent.change(screen.getByPlaceholderText('0.00'), { target: { value: '2999' } })
    fireEvent.click(screen.getByRole('button', { name: /下一步/ }))

    fireEvent.click(screen.getByRole('button', { name: '确认发布' }))

    await waitFor(() => {
      expect(apiMocks.publishCheck).toHaveBeenCalledWith('90000000001')
    })
    expect(apiMocks.itemPublish).not.toHaveBeenCalled()
    expect(notify).toHaveBeenCalledWith('账号暂不可发布', 'error')
  })

  it('blocks submit when badwords check rejects the generated title', async () => {
    apiMocks.publishBadwords.mockResolvedValueOnce({
      ret: ['SUCCESS::调用成功'],
      data: { safe: false, matched: ['微信'] },
    })
    renderSheet()

    fireEvent.change(screen.getByPlaceholderText('直接输入商品描述，发布时会自动生成标题'), {
      target: { value: '支持微信联系，苹果13 128G 国行蓝色。' },
    })
    const fileInput = document.body.querySelector('input[type="file"]')
    fireEvent.change(fileInput!, { target: { files: [new File(['fake image'], 'phone.jpg', { type: 'image/jpeg' })] } })
    await waitFor(() => expect(apiMocks.uploadItemImage).toHaveBeenCalled())
    fireEvent.click(screen.getByRole('button', { name: /下一步/ }))
    fireEvent.change(screen.getByPlaceholderText('0.00'), { target: { value: '2999' } })
    fireEvent.click(screen.getByRole('button', { name: /下一步/ }))

    fireEvent.click(screen.getByRole('button', { name: '确认发布' }))

    await waitFor(() => {
      expect(apiMocks.publishBadwords).toHaveBeenCalledWith('90000000001', '支持微信联系')
    })
    expect(apiMocks.itemPublish).not.toHaveBeenCalled()
    expect(notify).toHaveBeenCalledWith('标题包含疑似敏感词：微信', 'error')
  })

  it('preserves imported virtual-service and no-bargain flags on submit', async () => {
    apiMocks.itemImportLink.mockResolvedValue({
      ret: ['SUCCESS::调用成功'],
      data: {
        title: '远程装机服务',
        desc: '远程装机，支持软件安装与环境配置。',
        price: '88',
        imageUrls: ['https://img.alicdn.com/service.jpg'],
        simpleItem: true,
        noBargain: true,
      },
    })

    renderSheet()

    fireEvent.change(screen.getByPlaceholderText('粘贴闲鱼/淘宝商品链接或口令'), {
      target: { value: 'https://www.goofish.com/item?id=10001' },
    })
    fireEvent.click(screen.getByRole('button', { name: '导入' }))

    await waitFor(() => {
      expect(apiMocks.itemImportLink).toHaveBeenCalledWith('90000000001', 'https://www.goofish.com/item?id=10001')
    })

    fireEvent.click(screen.getByRole('button', { name: /下一步/ }))
    fireEvent.click(screen.getByRole('button', { name: /下一步/ }))
    fireEvent.click(screen.getByRole('button', { name: '确认发布' }))

    await waitFor(() => {
      expect(apiMocks.itemPublish).toHaveBeenCalledWith('90000000001', expect.objectContaining({
        simpleItem: true,
        noBargain: true,
      }))
    })
  })
})
