const DICEBEAR_STYLES = [
  'miniavs',
  'micah',
  'lorelei-neutral',
  'notionists-neutral',
  'bottts-neutral',
  'thumbs',
  'pixel-art-neutral',
  'adventurer-neutral',
  'avataaars-neutral',
  'open-peeps',
  'personas',
  'toon-head',
]

const BACKGROUND_COLORS = [
  'b6e3f4',
  'c0aede',
  'd1d4f9',
  'ffd5dc',
  'ffdfbf',
]

export function generatedAvatarDataUri(seed: string | null | undefined): string {
  const normalized = String(seed || 'xianyu-user').trim() || 'xianyu-user'
  const style = DICEBEAR_STYLES[hashSeed(normalized) % DICEBEAR_STYLES.length]
  const params = new URLSearchParams({
    seed: normalized,
    size: '256',
    radius: '50',
    backgroundColor: BACKGROUND_COLORS.join(','),
  })
  return `https://api.dicebear.com/9.x/${style}/png?${params.toString()}`
}

export const dicebearAvatarStyles = DICEBEAR_STYLES

function hashSeed(seed: string): number {
  let hash = 2166136261
  for (let i = 0; i < seed.length; i += 1) {
    hash ^= seed.charCodeAt(i)
    hash = Math.imul(hash, 16777619)
  }
  return hash >>> 0
}
