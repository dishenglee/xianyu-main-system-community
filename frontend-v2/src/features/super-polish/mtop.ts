import { isSuccess, retMsg, retStr } from '../../api/client'

const SESSION_EXPIRED_PATTERN = /(FAIL_SYS_SESSION_EXPIRED|SESSION_EXPIRED|SESSION过期|会话过期|会话已过期|NOT_LOGIN|请重新登录)/i

function hasRetField(value: unknown): value is { ret?: unknown } {
  return Boolean(value)
    && typeof value === 'object'
    && !Array.isArray(value)
    && Object.prototype.hasOwnProperty.call(value, 'ret')
}

export function getSuperPolishRetError(
  payload: unknown,
  fallback = '超级擦亮数据读取失败',
): string {
  if (!hasRetField(payload) || isSuccess(payload)) {
    return ''
  }

  const rawRet = retStr(payload)
  if (SESSION_EXPIRED_PATTERN.test(rawRet)) {
    return '账号会话已过期，无法读取超级擦亮数据，请先刷新账号后重试。'
  }

  return retMsg(payload, fallback)
}

export function assertSuperPolishRequestSuccess<T>(payload: T): T {
  const error = getSuperPolishRetError(payload)
  if (error) {
    throw new Error(error)
  }
  return payload
}
