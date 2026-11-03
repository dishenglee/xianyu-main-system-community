// @ts-nocheck
import { normalizeSessionTimestamp } from './sessionState'

function messageKey(message) {
  return message?.messageId || `${message?.version || 0}_${message?.ts || 0}_${message?.preview || ''}`
}

function normalizeHistoryMessage(message = {}) {
  const normalizedTs = normalizeSessionTimestamp(message?.ts) || normalizeSessionTimestamp(message?.version)
  return normalizedTs
    ? {
        ...message,
        ts: normalizedTs,
      }
    : { ...message }
}

// Drop optimistic_* placeholders when a real isSelf echo with matching
// signature (text + imageUrl) arrives within OPTIMISTIC_ECHO_WINDOW_MS.
// Must run AFTER the messageKey-based merge — optimistic messages have a
// different messageId than the server-side echo so the basic Map de-dup
// keeps both around. Without this pass, every outgoing text/image appears
// twice in the chat panel.
const OPTIMISTIC_ECHO_WINDOW_MS = 30_000
function messageSignature(m) {
  const meta = m && typeof m.cardMeta === 'object' && m.cardMeta !== null ? m.cardMeta : null
  const contentType = Number(m?.contentType ?? m?.content_type ?? 0)
  const cardKind = String(meta?.kind || '').trim()
  if (cardKind === 'location' || contentType === 30) {
    const title = String(meta?.title ?? m?.title ?? m?.text ?? m?.content ?? '').trim()
    const content = String(meta?.content ?? m?.content ?? '').trim()
    return `card:location|${title}|${content}`
  }
  if (cardKind === 'item' || contentType === 7 || contentType === 8) {
    const itemId = String(meta?.itemId ?? m?.itemId ?? '').trim()
    const title = String(meta?.title ?? m?.title ?? m?.text ?? m?.content ?? '').trim()
    const mainPic = String(meta?.mainPic ?? m?.imageUrl ?? '').trim()
    const price = String(meta?.price ?? '').trim()
    return `card:item|${itemId}|${title}|${mainPic}|${price}`
  }
  return `${String(m?.text ?? m?.content ?? '')}|${String(m?.imageUrl ?? '')}`
}
function dropOptimisticEchoes(messages) {
  if (!messages || messages.length < 2) return messages
  // First pass: collect signatures of REAL outgoing messages (any non-optimistic
  // is_self message that has a "real" id). Used both to drop optimistic
  // placeholders AND to dedup multiple real echoes of the same send (Xianyu's
  // ACCS occasionally fans out the same message twice with DIFFERENT
  // server-side messageIds, which `Map`-by-messageId can't catch).
  const realIsSelf = messages.filter((m) => {
    const id = String(m?.messageId || '')
    if (id.startsWith('optimistic_')) return false
    return Boolean(m?.isSelf)
  })
  if (!realIsSelf.length) return messages

  // Build a multimap signature -> [first-seen-ts]. Second/third copy of the
  // same signature within the echo window gets dropped.
  const firstSeenBySig = new Map()
  const keep = []
  for (const m of messages) {
    const id = String(m?.messageId || '')
    const isOpt = id.startsWith('optimistic_')
    if (!m?.isSelf && !isOpt) {
      // Incoming messages aren't subject to optimistic dedup; pass through.
      keep.push(m)
      continue
    }
    const sig = messageSignature(m)
    const ts = Number(m?.ts) || 0
    // Find a real-isSelf message (could be `m` itself) whose signature matches
    // and whose ts is within the echo window. If `m` is optimistic, look for
    // a NON-optimistic match. If `m` is a real isSelf, we still need to drop
    // duplicates of itself.
    const matched = realIsSelf.find((real) => {
      if (real === m) return false
      if (messageSignature(real) !== sig) return false
      const realTs = Number(real?.ts) || 0
      return Math.abs(realTs - ts) <= OPTIMISTIC_ECHO_WINDOW_MS
    })
    if (!matched) {
      keep.push(m)
      continue
    }
    if (isOpt) {
      // Drop optimistic — the real echo will represent it.
      continue
    }
    // Two real isSelf messages with same signature within the window:
    // keep only one. 默认按 ts 比，更早的赢；ts 相等（很常见——optimistic 用
    // Date.now()，loadHistory + SSE 都拿同一份服务端 createAt，正好撞同一毫秒）
    // 时按 messageId 字典序当 tiebreaker。否则 `>` 严格比较平局两条都进 keep，
    // 用户看到两个一模一样的 bubble。
    const matchedTs = Number(matched?.ts) || 0
    if (ts !== matchedTs) {
      if (ts > matchedTs) continue
      keep.push(m)
      continue
    }
    const myId = String(m?.messageId || '')
    const otherId = String(matched?.messageId || '')
    if (myId > otherId) continue
    keep.push(m)
  }
  // Final pass: dedup is_self messages that match BOTH on signature AND within
  // the echo window — this catches the case where ACCS occasionally re-pushes
  // the same outgoing message under two different server-side messageIds (the
  // Map-by-messageId merge keeps both, so without this pass the chat shows the
  // same line twice).
  //
  // Earlier versions of this pass deduped on signature alone with no time
  // check. That was wrong: when the user genuinely sends the same text twice
  // (e.g. "你好" at 10:04:59 then again at 10:05:43), the main loop above
  // keeps both (they're 44s apart, outside the 30s echo window), but a
  // signature-only final pass then collapsed them into one and the second
  // send appeared to vanish. Adding the same 30s window here fixes that.
  const finalKept = []
  const droppedSet = new Set()
  for (let i = 0; i < keep.length; i += 1) {
    if (droppedSet.has(i)) continue
    const m = keep[i]
    if (!m?.isSelf) {
      finalKept.push(m)
      continue
    }
    const sig = messageSignature(m)
    const ts = Number(m?.ts) || 0
    // Scan AHEAD for a same-signature is_self message within the echo window.
    // If found, pick a winner (prefer non-optimistic, then earlier ts) and
    // mark the loser dropped via index so we don't visit it later.
    let winnerIdx = i
    let winner = m
    for (let j = i + 1; j < keep.length; j += 1) {
      if (droppedSet.has(j)) continue
      const other = keep[j]
      if (!other?.isSelf) continue
      if (messageSignature(other) !== sig) continue
      const otherTs = Number(other?.ts) || 0
      if (Math.abs(otherTs - ts) > OPTIMISTIC_ECHO_WINDOW_MS) continue
      const winnerOpt = String(winner?.messageId || '').startsWith('optimistic_')
      const otherOpt = String(other?.messageId || '').startsWith('optimistic_')
      const winnerTs = Number(winner?.ts) || 0
      const otherWins =
        (winnerOpt && !otherOpt) ||
        (winnerOpt === otherOpt && otherTs > 0 && (winnerTs === 0 || otherTs < winnerTs))
      if (otherWins) {
        droppedSet.add(winnerIdx)
        winnerIdx = j
        winner = other
      } else {
        droppedSet.add(j)
      }
    }
    finalKept.push(winner)
  }
  return finalKept
}

function mergeMessageLists(leftMessages = [], rightMessages = []) {
  const merged = new Map()
  for (const message of [...leftMessages, ...rightMessages]) {
    const normalized = normalizeHistoryMessage(message)
    merged.set(messageKey(normalized), normalized)
  }
  const sorted = Array.from(merged.values()).sort((left, right) => {
    const leftOrder = normalizeSessionTimestamp(left.version) || normalizeSessionTimestamp(left.ts)
    const rightOrder = normalizeSessionTimestamp(right.version) || normalizeSessionTimestamp(right.ts)
    if (leftOrder !== rightOrder) {
      return leftOrder - rightOrder
    }
    return (normalizeSessionTimestamp(left.ts) || 0) - (normalizeSessionTimestamp(right.ts) || 0)
  })
  return dropOptimisticEchoes(sorted)
}

function toPositiveNumber(value) {
  const number = Number(value || 0)
  return Number.isFinite(number) && number > 0 ? number : 0
}

function minPositive(...values) {
  const numbers = values.map(toPositiveNumber).filter(Boolean)
  return numbers.length ? Math.min(...numbers) : 0
}

function maxPositive(...values) {
  const numbers = values.map(toPositiveNumber).filter(Boolean)
  return numbers.length ? Math.max(...numbers) : 0
}

export function mergeHistoryState(existing = {}, incoming = {}, mode = 'replace') {
  if (mode === 'older') {
    return {
      ...existing,
      ...incoming,
      nextCursor: minPositive(existing.nextCursor, incoming.nextCursor),
      latestCursor: maxPositive(existing.latestCursor, incoming.latestCursor),
      messages: mergeMessageLists(incoming.messages || [], existing.messages || []),
    }
  }

  if (mode === 'refresh') {
    return {
      ...existing,
      ...incoming,
      nextCursor: minPositive(existing.nextCursor, incoming.nextCursor),
      latestCursor: maxPositive(existing.latestCursor, incoming.latestCursor),
      messages: mergeMessageLists(existing.messages || [], incoming.messages || []),
    }
  }

  return {
    ...existing,
    ...incoming,
    nextCursor: toPositiveNumber(incoming.nextCursor),
    latestCursor: maxPositive(incoming.latestCursor),
    messages: (incoming.messages || []).map((message) => normalizeHistoryMessage(message)),
  }
}

/* UNIQUE_TEST_MARKER_DEDUP_2026XYZ */
