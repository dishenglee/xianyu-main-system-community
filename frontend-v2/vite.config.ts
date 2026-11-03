import { defineConfig } from 'vitest/config'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import { createCipheriv, randomBytes } from 'node:crypto'
import { resolve } from 'path'

const PROTOCOL_PROPERTY_NAME_SOURCE = String.raw`(?:[A-Za-z0-9_$]*(?:itemDO)[A-Za-z0-9_$]*|contentParams|mainParams|loginFormData)`
const PROTOCOL_NETWORK_LITERAL_SOURCE = String.raw`(?:\/api\/[A-Za-z0-9_./:-]+|https?:\/\/(?:[A-Za-z0-9-]+\.)*goo?ofish\.(?:shop|com)(?:\/[A-Za-z0-9_./:?&=%-]*)?|x-(?:device|license|yuguanjia)-[A-Za-z0-9-]+|mtop\.(?:taobao|alibaba|idle|wx)\.[A-Za-z0-9._-]+)`
const PROTOCOL_LITERAL_SOURCE = String.raw`(?:${PROTOCOL_NETWORK_LITERAL_SOURCE}|${PROTOCOL_PROPERTY_NAME_SOURCE})`
const PROTOCOL_TEMPLATE_LITERAL_RE = new RegExp(PROTOCOL_NETWORK_LITERAL_SOURCE, 'gi')
const PROTOCOL_QUOTED_LITERAL_RE = new RegExp(
  String.raw`"([^"` + "`" + String.raw`\\]*${PROTOCOL_LITERAL_SOURCE}[^"` + "`" + String.raw`\\]*)"|'([^'` + "`" + String.raw`\\]*${PROTOCOL_LITERAL_SOURCE}[^'` + "`" + String.raw`\\]*)'`,
  'gi',
)
const PROTOCOL_TEMPLATE_FIELD_RE = /`([A-Za-z0-9_$]*(?:itemDO)[A-Za-z0-9_$]*|contentParams|mainParams|loginFormData)`/gi
const PROTOCOL_QUOTED_OBJECT_KEY_RE = new RegExp(
  String.raw`([,{]\s*)(["'])(` + PROTOCOL_LITERAL_SOURCE + String.raw`)(\2)\s*:`,
  'gi',
)
const PROTOCOL_OPTIONAL_PROPERTY_RE = new RegExp(`\\?\\.(${PROTOCOL_PROPERTY_NAME_SOURCE})\\b`, 'gi')
const PROTOCOL_PROPERTY_RE = new RegExp(`\\.(${PROTOCOL_PROPERTY_NAME_SOURCE})\\b`, 'gi')
const PROTOCOL_OBJECT_KEY_RE = new RegExp(`([,{]\\s*)(${PROTOCOL_PROPERTY_NAME_SOURCE})\\s*:`, 'gi')

type SealedProtocolLiteral = {
  nonce: number[]
  ciphertext: number[]
}

function encryptProtocolLiteral(value: string, key: Buffer): SealedProtocolLiteral {
  const nonce = randomBytes(12)
  const cipher = createCipheriv('aes-256-gcm', key, nonce)
  const ciphertext = Buffer.concat([
    cipher.update(value, 'utf8'),
    cipher.final(),
    cipher.getAuthTag(),
  ])
  return {
    nonce: Array.from(nonce),
    ciphertext: Array.from(ciphertext),
  }
}

function protocolRuntimeBootstrap(key: Buffer, values: SealedProtocolLiteral[]): string {
  const encryptedValues = values.map(({ nonce, ciphertext }) => [nonce, ciphertext])
  return `const __protocol=await(async()=>{const k=await globalThis.crypto.subtle.importKey("raw",new Uint8Array(${JSON.stringify(Array.from(key))}),{name:"AES-GCM"},false,["decrypt"]);const d=new TextDecoder;const v=await Promise.all(${JSON.stringify(encryptedValues)}.map(async([n,c])=>d.decode(await globalThis.crypto.subtle.decrypt({name:"AES-GCM",iv:new Uint8Array(n)},k,new Uint8Array(c)))));return i=>v[i]})();`
}

function sealFrontendProtocolLiterals() {
  return {
    name: 'seal-frontend-protocol-literals',
    generateBundle(_options: unknown, bundle: Record<string, any>) {
      for (const asset of Object.values(bundle)) {
        if (asset.type !== 'chunk' || typeof asset.code !== 'string') continue
        let changed = false
        let code = asset.code
        const key = randomBytes(32)
        const encrypted: SealedProtocolLiteral[] = []
        const indexes = new Map<string, number>()
        const sealedCall = (value: string): string => {
          let index = indexes.get(value)
          if (index === undefined) {
            index = encrypted.length
            indexes.set(value, index)
            encrypted.push(encryptProtocolLiteral(value, key))
          }
          changed = true
          return `__protocol(${index})`
        }
        code = code.replace(
          PROTOCOL_QUOTED_OBJECT_KEY_RE,
          (_match: string, prefix: string, _quote: string, value: string) =>
            `${prefix}[${sealedCall(value)}]:`,
        )
        code = code.replace(PROTOCOL_QUOTED_LITERAL_RE, (_match: string, doubleValue: string, singleValue: string) => {
          return sealedCall(doubleValue ?? singleValue)
        })
        code = code.replace(PROTOCOL_TEMPLATE_FIELD_RE, (_match: string, value: string) => sealedCall(value))
        code = code.replace(/`([^`\\]*(?:\\.[^`\\]*)*)`/gs, (template: string, body: string) => {
          const sealed = body.replace(PROTOCOL_TEMPLATE_LITERAL_RE, (value: string) => {
            return `\${${sealedCall(value)}}`
          })
          return sealed === body ? template : `\`${sealed}\``
        })
        code = code.replace(PROTOCOL_OPTIONAL_PROPERTY_RE, (_match: string, property: string) => {
          return `?.[${sealedCall(property)}]`
        })
        code = code.replace(PROTOCOL_PROPERTY_RE, (_match: string, property: string) => {
          return `[${sealedCall(property)}]`
        })
        code = code.replace(PROTOCOL_OBJECT_KEY_RE, (_match: string, prefix: string, property: string) => {
          return `${prefix}[${sealedCall(property)}]:`
        })
        if (changed) {
          asset.code = `${protocolRuntimeBootstrap(key, encrypted)}${code}`
        }
      }
    },
  }
}

export default defineConfig(({ mode }) => {
  const isProduction = mode === 'production'
  const outDir = process.env.YUGUANJIA_STATIC_DIR || process.env.RELEASE_FRONTEND_DIST_DIR || '../static'

  return {
    base: isProduction ? './' : '/',
    plugins: [
      react(),
      tailwindcss(),
      sealFrontendProtocolLiterals(),
    ],
    resolve: {
      alias: {
        '@': resolve(__dirname, './src'),
      },
    },
    build: {
      outDir,
      emptyOutDir: true,
      sourcemap: false,
      minify: 'oxc',
    },
    server: {
      proxy: {
        '/api': {
          target: 'http://localhost:3000',
          ws: true,
          changeOrigin: true,
        },
      },
    },
    test: {
      environment: 'jsdom',
      globals: true,
      setupFiles: [],
    },
  }
})
