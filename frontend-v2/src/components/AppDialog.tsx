import React, {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
} from 'react'
import { Modal } from './Modal'
import { dialogMessageTextStyle } from './dialogTypography'
import { useSafeNotify } from './Notify'

type DialogMessage = React.ReactNode

type BaseOptions = {
  title?: string
  confirmLabel?: string
  cancelLabel?: string
  danger?: boolean
  width?: number
}

type AlertOptions = Pick<BaseOptions, 'title' | 'confirmLabel' | 'width'>

type ConfirmOptions = BaseOptions

type PromptOptions = BaseOptions & {
  defaultValue?: string
  placeholder?: string
}

type AlertDialogState = {
  kind: 'alert'
  title?: string
  message: DialogMessage
  confirmLabel?: string
  width?: number
  resolve: () => void
}

type ConfirmDialogState = {
  kind: 'confirm'
  title?: string
  message: DialogMessage
  confirmLabel?: string
  cancelLabel?: string
  danger?: boolean
  width?: number
  resolve: (confirmed: boolean) => void
}

type PromptDialogState = {
  kind: 'prompt'
  title?: string
  message: DialogMessage
  confirmLabel?: string
  cancelLabel?: string
  placeholder?: string
  defaultValue?: string
  danger?: boolean
  width?: number
  resolve: (value: string | null) => void
}

type DialogState = AlertDialogState | ConfirmDialogState | PromptDialogState

type AppDialogApi = {
  showAlert: (message: DialogMessage, options?: AlertOptions) => Promise<void>
  showConfirm: (message: DialogMessage, options?: ConfirmOptions) => Promise<boolean>
  showPrompt: (message: DialogMessage, options?: PromptOptions) => Promise<string | null>
}

const AppDialogContext = createContext<AppDialogApi | null>(null)

function defaultTitle(kind: DialogState['kind']) {
  if (kind === 'confirm') return '请确认'
  if (kind === 'prompt') return '请输入'
  return '操作提示'
}

export function AppDialogProvider({ children }: { children: React.ReactNode }) {
  const [dialog, setDialog] = useState<DialogState | null>(null)
  const [promptValue, setPromptValue] = useState('')
  const notify = useSafeNotify()

  useEffect(() => {
    if (dialog?.kind === 'prompt') {
      setPromptValue(dialog.defaultValue ?? '')
      return
    }
    setPromptValue('')
  }, [dialog])

  const showAlert = useCallback((message: DialogMessage, _options: AlertOptions = {}) => {
    const text = typeof message === 'string' ? message : '操作提示'
    const type = /失败|错误|异常|无效|不能|不合法|未拿到|没有|请输入|请选择|请先|仍有/.test(text)
      ? 'error'
      : /成功|完成|已开启|已关闭|已修改|已更新|已复制|已创建|已删除/.test(text)
        ? 'success'
        : 'info'
    notify(text, type, type === 'error' ? 4200 : 3000)
    return Promise.resolve()
  }, [notify])

  const showConfirm = useCallback((message: DialogMessage, options: ConfirmOptions = {}) => {
    return new Promise<boolean>((resolve) => {
      setDialog({
        kind: 'confirm',
        title: options.title,
        message,
        confirmLabel: options.confirmLabel,
        cancelLabel: options.cancelLabel,
        danger: options.danger,
        width: options.width,
        resolve,
      })
    })
  }, [])

  const showPrompt = useCallback((message: DialogMessage, options: PromptOptions = {}) => {
    return new Promise<string | null>((resolve) => {
      setDialog({
        kind: 'prompt',
        title: options.title,
        message,
        confirmLabel: options.confirmLabel,
        cancelLabel: options.cancelLabel,
        placeholder: options.placeholder,
        defaultValue: options.defaultValue ?? '',
        danger: options.danger,
        width: options.width,
        resolve,
      })
    })
  }, [])

  const closeWithResult = useCallback((result?: boolean | string | null) => {
    if (!dialog) return
    const current = dialog
    setDialog(null)
    if (current.kind === 'confirm') {
      current.resolve(result === true)
      return
    }
    if (current.kind === 'prompt') {
      current.resolve(typeof result === 'string' ? result : null)
      return
    }
    current.resolve()
  }, [dialog])

  const handleClose = useCallback(() => {
    if (!dialog) return
    if (dialog.kind === 'prompt') {
      closeWithResult(null)
      return
    }
    if (dialog.kind === 'confirm') {
      closeWithResult(false)
      return
    }
    closeWithResult()
  }, [closeWithResult, dialog])

  const handleConfirm = useCallback(() => {
    if (!dialog) return
    if (dialog.kind === 'prompt') {
      closeWithResult(promptValue)
      return
    }
    if (dialog.kind === 'confirm') {
      closeWithResult(true)
      return
    }
    closeWithResult()
  }, [closeWithResult, dialog, promptValue])

  const api = useMemo<AppDialogApi>(() => ({
    showAlert,
    showConfirm,
    showPrompt,
  }), [showAlert, showConfirm, showPrompt])

  const footer = dialog ? (
    <>
      {dialog.kind !== 'alert' && (
        <button
          type="button"
          onClick={handleClose}
          className="btn-secondary btn-sm flex-1 sm:flex-none"
        >
          {dialog.cancelLabel || '取消'}
        </button>
      )}
      <button
        type="button"
        onClick={handleConfirm}
        className={`${dialog.kind !== 'alert' && dialog.danger ? 'btn-danger' : 'btn-primary'} btn-sm flex-1 sm:flex-none`}
      >
        {dialog.confirmLabel || '确认'}
      </button>
    </>
  ) : null

  return (
    <AppDialogContext.Provider value={api}>
      {children}
      <Modal
        open={Boolean(dialog)}
        onClose={handleClose}
        title={dialog ? (dialog.title || defaultTitle(dialog.kind)) : ''}
        footer={footer}
        width={dialog?.width || 420}
      >
        {dialog && (
          <div className="space-y-4">
            <div
              className="text-center font-semibold whitespace-pre-wrap"
              style={{ color: 'var(--text-main)', ...dialogMessageTextStyle(dialog.message) }}
            >
              {dialog.message}
            </div>
            {dialog.kind === 'prompt' && (
              <input
                autoFocus
                className="input"
                value={promptValue}
                onChange={(event) => setPromptValue(event.target.value)}
                placeholder={dialog.placeholder || ''}
                onKeyDown={(event) => {
                  if (event.key === 'Enter') handleConfirm()
                }}
              />
            )}
          </div>
        )}
      </Modal>
    </AppDialogContext.Provider>
  )
}

export function useAppDialog() {
  const context = useContext(AppDialogContext)
  if (!context) {
    throw new Error('useAppDialog must be used within AppDialogProvider')
  }
  return context
}
