export async function limitConcurrency<T, R>(
  items: T[],
  limit: number,
  worker: (item: T, index: number) => Promise<R>,
): Promise<R[]> {
  const list = Array.isArray(items) ? items : []
  const maxConcurrent = Math.max(1, Number(limit || 1))
  const results = new Array<R>(list.length)
  let nextIndex = 0

  async function runOne() {
    while (nextIndex < list.length) {
      const currentIndex = nextIndex
      nextIndex += 1
      results[currentIndex] = await worker(list[currentIndex], currentIndex)
    }
  }

  const runners = Array.from(
    { length: Math.min(maxConcurrent, list.length || 1) },
    () => runOne(),
  )
  await Promise.all(runners)
  return results
}
