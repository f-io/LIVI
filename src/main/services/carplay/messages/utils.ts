export const clamp = (number: number, min: number, max: number) => {
  return Math.max(min, Math.min(number, max))
}

export function getCurrentTimeInMs() {
  return Math.round(Date.now() / 1000)
}
