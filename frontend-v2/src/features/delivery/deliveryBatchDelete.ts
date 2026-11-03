export function getDeliverySelectionState(selectedIds: Set<number>, ids: number[]) {
  const selectedCount = ids.reduce((count, id) => count + (selectedIds.has(id) ? 1 : 0), 0)
  return {
    checked: ids.length > 0 && selectedCount === ids.length,
    indeterminate: selectedCount > 0 && selectedCount < ids.length,
  }
}

export function toggleDeliverySelection(
  selectedIds: Set<number>,
  ids: number[],
  checked: boolean,
) {
  const next = new Set(selectedIds)
  ids.forEach((id) => checked ? next.add(id) : next.delete(id))
  return next
}

function failureReason(cause: unknown) {
  if (cause instanceof Error) return cause.message || '删除失败'
  return String(cause || '删除失败')
}

export async function runDeliveryBatchDelete(
  ids: number[],
  remove: (id: number) => Promise<unknown>,
  concurrency = 3,
) {
  const targets = Array.from(new Set(ids))
  const results: Array<{ id: number; reason: string } | null> = new Array(targets.length).fill(null)
  let cursor = 0

  async function worker() {
    while (cursor < targets.length) {
      const index = cursor
      cursor += 1
      const id = targets[index]
      try {
        await remove(id)
      } catch (cause) {
        results[index] = { id, reason: failureReason(cause) }
      }
    }
  }

  await Promise.all(Array.from(
    { length: Math.min(Math.max(1, concurrency), targets.length) },
    () => worker(),
  ))

  const failures = results.filter((result): result is { id: number; reason: string } => Boolean(result))
  const failedIds = new Set(failures.map((failure) => failure.id))
  return {
    succeededIds: targets.filter((id) => !failedIds.has(id)),
    failures,
  }
}
