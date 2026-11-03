// Previously a rainbow palette to give each account a unique ring colour.
// Dropped per user feedback ("各种颜色乱飞") — every account now maps to the
// same neutral Apple system gray so the UI stays minimal. Account disambi-
// guation is done via the real avatar / nick label, not colour.
const NEUTRAL = '#C7C7CC'

export function colorForAccount(_uid: string): string {
  return NEUTRAL
}
