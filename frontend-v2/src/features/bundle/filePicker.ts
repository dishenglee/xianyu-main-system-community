const ACCOUNT_DATA_IMPORT_ACCEPT_DESKTOP = '.ygjdata,application/zip,application/octet-stream,application/x-zip-compressed'
const ACCOUNT_DATA_IMPORT_ACCEPT_ANDROID = '*/*'

function readUserAgent() {
  if (typeof navigator === 'undefined') return ''
  return navigator.userAgent || ''
}

export function getAccountDataImportAccept(userAgent = readUserAgent()) {
  // Android document providers may gray out custom extensions when accept is too narrow.
  if (/\bAndroid\b/i.test(userAgent)) return ACCOUNT_DATA_IMPORT_ACCEPT_ANDROID
  return ACCOUNT_DATA_IMPORT_ACCEPT_DESKTOP
}
