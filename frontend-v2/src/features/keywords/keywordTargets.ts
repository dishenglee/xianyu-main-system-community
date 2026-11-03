export function resolveKeywordTargetUids(selectedUids: readonly string[] = []) {
  const seen = new Set<string>()
  const result: string[] = []

  for (const uid of selectedUids) {
    const normalized = String(uid || '').trim()
    if (!normalized || seen.has(normalized)) continue
    seen.add(normalized)
    result.push(normalized)
  }

  return result
}

type KeywordTemplate = {
  id?: number
  keyword?: string
  reply?: string
  type?: string
  itemId?: string
  item_id?: string
  enabled?: boolean
  cookieId?: string
  cookie_id?: string
  taskGroupId?: string
  task_group_id?: string
  createdAt?: number
  created_at?: number
}

export type AccountKeywordTaskEntry<T extends KeywordTemplate = KeywordTemplate> = {
  key: string
  keyword: string
  itemId: string
  reply: string
  type: string
  enabled: boolean
  ruleIds: number[]
  accountUids: string[]
  records: T[]
}

export type AccountKeywordTask<T extends KeywordTemplate = KeywordTemplate> = {
  key: string
  taskGroupId: string
  enabled: boolean
  ruleIds: number[]
  accountUids: string[]
  entries: AccountKeywordTaskEntry<T>[]
  records: T[]
}

export function keywordTemplateKey(rule: KeywordTemplate) {
  return [
    String(rule.keyword || '').trim().toLowerCase(),
    String(rule.itemId || rule.item_id || '').trim(),
    String(rule.type || 'text').trim() || 'text',
  ].join('\u0000')
}

export function selectSavedKeywordTemplates<T extends KeywordTemplate>(
  templates: T[] = [],
  selectedIds: readonly number[] = [],
) {
  const selected = new Set(selectedIds.map((id) => Number(id)).filter((id) => Number.isFinite(id)))
  return templates.filter((template) => {
    const id = Number(template.id)
    if (!Number.isFinite(id) || !selected.has(id)) return false
    if (!template.enabled) return false
    return Boolean(String(template.keyword || '').trim() && String(template.reply || '').trim())
  })
}

export function buildKeywordPayload(template: KeywordTemplate) {
  return {
    keyword: String(template.keyword || '').trim(),
    reply: String(template.reply || '').trim(),
    type: String(template.type || 'text').trim() || 'text',
    itemId: String(template.itemId || template.item_id || '').trim(),
    enabled: true,
  }
}

export function buildAccountKeywordPayloads(
  templates: KeywordTemplate[] = [],
  options: {
    taskGroupId?: string
  } = {},
) {
  const taskGroupId = String(options.taskGroupId || '').trim()
  return templates.map((template) => ({
    ...buildKeywordPayload(template),
    taskGroupId,
  }))
}

export function groupAccountKeywordTasks<T extends KeywordTemplate>(
  keywords: T[] = [],
): AccountKeywordTask<T>[] {
  const groups = new Map<
    string,
    {
      key: string
      taskGroupId: string
      ruleIds: number[]
      accountUids: string[]
      records: T[]
      entryMap: Map<string, AccountKeywordTaskEntry<T>>
      latestCreatedAt: number
    }
  >()

  for (const keywordRule of keywords) {
    const keyword = String(keywordRule.keyword || '').trim()
    const itemId = String(keywordRule.itemId || keywordRule.item_id || '').trim()
    const reply = String(keywordRule.reply || '').trim()
    const type = String(keywordRule.type || 'text').trim() || 'text'
    const enabled = Boolean(keywordRule.enabled)
    const accountUid = String(keywordRule.cookieId || keywordRule.cookie_id || '').trim()
    const taskGroupId = String(keywordRule.taskGroupId || keywordRule.task_group_id || '').trim()
    const createdAt = Number(keywordRule.createdAt ?? keywordRule.created_at ?? 0) || 0
    const ruleId = Number(keywordRule.id)
    const entryKey = [
      keyword.toLowerCase(),
      itemId,
      reply,
      type,
      enabled ? '1' : '0',
    ].join('\u0001')
    const key = taskGroupId || `legacy:${entryKey}`

    if (!groups.has(key)) {
      groups.set(key, {
        key,
        taskGroupId,
        ruleIds: [],
        accountUids: [],
        records: [],
        entryMap: new Map<string, AccountKeywordTaskEntry<T>>(),
        latestCreatedAt: createdAt,
      })
    }

    const group = groups.get(key)!
    group.latestCreatedAt = Math.max(group.latestCreatedAt, createdAt)
    if (Number.isFinite(ruleId) && !group.ruleIds.includes(ruleId)) {
      group.ruleIds.push(ruleId)
    }
    if (accountUid && !group.accountUids.includes(accountUid)) {
      group.accountUids.push(accountUid)
    }
    group.records.push(keywordRule)

    if (!group.entryMap.has(entryKey)) {
      group.entryMap.set(entryKey, {
        key: entryKey,
        keyword,
        itemId,
        reply,
        type,
        enabled,
        ruleIds: [],
        accountUids: [],
        records: [],
      })
    }

    const entry = group.entryMap.get(entryKey)!
    if (Number.isFinite(ruleId) && !entry.ruleIds.includes(ruleId)) {
      entry.ruleIds.push(ruleId)
    }
    if (accountUid && !entry.accountUids.includes(accountUid)) {
      entry.accountUids.push(accountUid)
    }
    entry.records.push(keywordRule)
  }

  return Array.from(groups.values())
    .sort((left, right) => right.latestCreatedAt - left.latestCreatedAt)
    .map((group) => ({
      key: group.key,
      taskGroupId: group.taskGroupId,
      enabled: group.records.every((record) => Boolean(record.enabled)),
      ruleIds: group.ruleIds,
      accountUids: group.accountUids,
      entries: Array.from(group.entryMap.values()),
      records: group.records,
    }))
}

export function findExistingKeywordByTemplate<T extends KeywordTemplate>(
  existingRules: T[] = [],
  template: KeywordTemplate,
) {
  const targetKey = keywordTemplateKey(template)
  return existingRules.find((rule) => keywordTemplateKey(rule) === targetKey) || null
}
