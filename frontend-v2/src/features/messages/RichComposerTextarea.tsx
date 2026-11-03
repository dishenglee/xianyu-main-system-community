import {
  forwardRef,
  type CSSProperties,
  type KeyboardEvent,
  type TextareaHTMLAttributes,
  useMemo,
  useRef,
} from 'react'

import { containsXianyuEmoji, renderXianyuEmojiText } from './xianyuEmojiText'

type NativeTextareaProps = Omit<
  TextareaHTMLAttributes<HTMLTextAreaElement>,
  'value' | 'onChange'
>

interface RichComposerTextareaProps extends NativeTextareaProps {
  value: string
  onChange: (value: string) => void
  containerClassName?: string
  previewClassName?: string
  previewStyle?: CSSProperties
}

export const RichComposerTextarea = forwardRef<HTMLTextAreaElement, RichComposerTextareaProps>(
  function RichComposerTextarea({
    value,
    onChange,
    containerClassName,
    previewClassName,
    previewStyle,
    className,
    style,
    onScroll,
    onKeyDown,
    ...textareaProps
  }, forwardedRef) {
    const previewRef = useRef<HTMLDivElement | null>(null)
    const showPreview = useMemo(() => containsXianyuEmoji(value), [value])
    const textColor = style?.color || 'var(--text-main)'

    function setRefs(node: HTMLTextAreaElement | null) {
      if (typeof forwardedRef === 'function') {
        forwardedRef(node)
      } else if (forwardedRef) {
        forwardedRef.current = node
      }
    }

    return (
      <div className={containerClassName || 'relative w-full'} style={{ position: 'relative' }}>
        {showPreview ? (
          <div
            ref={previewRef}
            aria-label="消息输入预览"
            className={previewClassName}
            style={{
              position: 'absolute',
              inset: 0,
              zIndex: 0,
              pointerEvents: 'none',
              overflow: 'hidden',
              whiteSpace: 'pre-wrap',
              wordBreak: 'break-word',
              color: textColor,
              ...previewStyle,
            }}
          >
            {renderXianyuEmojiText(value)}
          </div>
        ) : null}
        <textarea
          {...textareaProps}
          ref={setRefs}
          value={value}
          onChange={(event) => onChange(event.target.value)}
          onScroll={(event) => {
            if (previewRef.current) {
              previewRef.current.scrollTop = event.currentTarget.scrollTop
              previewRef.current.scrollLeft = event.currentTarget.scrollLeft
            }
            onScroll?.(event)
          }}
          onKeyDown={(event: KeyboardEvent<HTMLTextAreaElement>) => onKeyDown?.(event)}
          className={className}
          style={{
            ...style,
            position: 'relative',
            zIndex: 1,
            ...(showPreview ? { background: 'transparent' } : null),
            color: showPreview ? 'transparent' : textColor,
            caretColor: textColor,
          }}
        />
      </div>
    )
  },
)
