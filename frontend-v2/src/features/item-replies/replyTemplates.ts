type ReplyTemplate = {
  id?: number;
  itemId?: string;
  item_id?: string;
  reply?: string;
  type?: string;
  enabled?: boolean;
  autoRecall?: boolean;
  auto_recall?: boolean;
  cookieId?: string;
  cookie_id?: string;
  taskGroupId?: string;
  task_group_id?: string;
  replyCount?: number;
  reply_count?: number;
  createdAt?: number;
  created_at?: number;
};

export function normalizeReplyCount(value: unknown) {
  const parsed = Number(value);
  if (!Number.isFinite(parsed)) return 1;
  return Math.max(1, Math.min(999, Math.trunc(parsed)));
}

export type AccountReplyTaskEntry<T extends ReplyTemplate = ReplyTemplate> = {
  key: string;
  itemId: string;
  reply: string;
  type: string;
  enabled: boolean;
  autoRecall: boolean;
  ruleIds: number[];
  accountUids: string[];
  records: T[];
};

export type AccountReplyTask<T extends ReplyTemplate = ReplyTemplate> = {
  key: string;
  taskGroupId: string;
  enabled: boolean;
  autoRecall: boolean;
  partialAutoRecall: boolean;
  replyCount: number;
  partialReplyCount: boolean;
  ruleIds: number[];
  accountUids: string[];
  entries: AccountReplyTaskEntry<T>[];
  records: T[];
};

export function selectSavedReplyTemplates<T extends ReplyTemplate>(
  replies: T[] = [],
  selectedIds: readonly number[] = [],
) {
  const selected = new Set(
    selectedIds.map((id) => Number(id)).filter((id) => Number.isFinite(id)),
  );
  return replies.filter((reply) => {
    const id = Number(reply.id);
    if (!Number.isFinite(id) || !selected.has(id)) return false;
    if (!reply.enabled) return false;
    return Boolean(String(reply.reply || "").trim());
  });
}

export function buildAccountItemReplyPayloads(
  replies: ReplyTemplate[] = [],
  options: {
    autoRecall?: boolean;
    taskGroupId?: string;
    replyCount?: number;
  } = {},
) {
  const taskGroupId = String(options.taskGroupId || "").trim();
  const autoRecall = Boolean(options.autoRecall);
  const replyCount = normalizeReplyCount(options.replyCount);
  return replies.map((reply) => ({
    itemId: String(reply.itemId || reply.item_id || "").trim(),
    reply: String(reply.reply || "").trim(),
    type: String(reply.type || "text"),
    enabled: Boolean(reply.enabled ?? true),
    autoRecall,
    taskGroupId,
    replyCount,
  }));
}

export function groupAccountReplyTasks<T extends ReplyTemplate>(
  replies: T[] = [],
): AccountReplyTask<T>[] {
  const groups = new Map<
    string,
    {
      key: string;
      taskGroupId: string;
      ruleIds: number[];
      accountUids: string[];
      records: T[];
      entryMap: Map<string, AccountReplyTaskEntry<T>>;
      latestCreatedAt: number;
    }
  >();

  for (const reply of replies) {
    const itemId = String(reply.itemId || reply.item_id || "").trim();
    const text = String(reply.reply || "").trim();
    const type = String(reply.type || "text").trim() || "text";
    const enabled = Boolean(reply.enabled);
    const autoRecall = Boolean(reply.autoRecall ?? reply.auto_recall);
    const replyCount = normalizeReplyCount(reply.replyCount ?? reply.reply_count);
    const accountUid = String(reply.cookieId || reply.cookie_id || "").trim();
    const taskGroupId = String(
      reply.taskGroupId || reply.task_group_id || "",
    ).trim();
    const createdAt = Number(reply.createdAt ?? reply.created_at ?? 0) || 0;
    const ruleId = Number(reply.id);
    const replyKey = [
      itemId,
      text,
      type,
      enabled ? "1" : "0",
      autoRecall ? "1" : "0",
    ].join("\u0001");
    const key = taskGroupId || `legacy:${replyKey}\u0001${replyCount}`;

    if (!groups.has(key)) {
      groups.set(key, {
        key,
        ruleIds: [],
        accountUids: [],
        records: [],
        taskGroupId,
        entryMap: new Map<string, AccountReplyTaskEntry<T>>(),
        latestCreatedAt: createdAt,
      });
    }

    const group = groups.get(key)!;
    group.latestCreatedAt = Math.max(group.latestCreatedAt, createdAt);
    if (Number.isFinite(ruleId) && !group.ruleIds.includes(ruleId)) {
      group.ruleIds.push(ruleId);
    }
    if (accountUid && !group.accountUids.includes(accountUid)) {
      group.accountUids.push(accountUid);
    }
    group.records.push(reply);

    if (!group.entryMap.has(replyKey)) {
      group.entryMap.set(replyKey, {
        key: replyKey,
        itemId,
        reply: text,
        type,
        enabled,
        autoRecall,
        ruleIds: [],
        accountUids: [],
        records: [],
      });
    }

    const entry = group.entryMap.get(replyKey)!;
    if (Number.isFinite(ruleId) && !entry.ruleIds.includes(ruleId)) {
      entry.ruleIds.push(ruleId);
    }
    if (accountUid && !entry.accountUids.includes(accountUid)) {
      entry.accountUids.push(accountUid);
    }
    entry.records.push(reply);
  }

  return Array.from(groups.values())
    .sort((left, right) => right.latestCreatedAt - left.latestCreatedAt)
    .map((group) => {
      const entries = Array.from(group.entryMap.values());
      const autoRecallCount = entries.filter(
        (entry) => entry.autoRecall,
      ).length;
      const replyCounts = entries.map((entry) =>
        normalizeReplyCount(entry.records[0]?.replyCount ?? entry.records[0]?.reply_count),
      );
      const normalizedReplyCount = replyCounts[0] ?? 1;
      return {
        key: group.key,
        taskGroupId: group.taskGroupId,
        enabled: entries.every((entry) => entry.enabled),
        autoRecall: autoRecallCount > 0 && autoRecallCount === entries.length,
        partialAutoRecall:
          autoRecallCount > 0 && autoRecallCount < entries.length,
        replyCount: normalizedReplyCount,
        partialReplyCount: replyCounts.some(
          (count) => count !== normalizedReplyCount,
        ),
        ruleIds: group.ruleIds,
        accountUids: group.accountUids,
        entries,
        records: group.records,
      };
    });
}

export function resolveReplyTargetUids(selectedUids: readonly string[] = []) {
  const seen = new Set<string>();
  const result: string[] = [];

  for (const uid of selectedUids) {
    const normalized = String(uid || "").trim();
    if (!normalized || seen.has(normalized)) continue;
    seen.add(normalized);
    result.push(normalized);
  }

  return result;
}
