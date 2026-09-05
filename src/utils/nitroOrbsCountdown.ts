export type NitroOrbsClaim = {
  value: number
  unit: 'days' | 'hours' | 'minutes'
}

/**
 * Convert Discord's absolute next-reward timestamp into the compact claim
 * value used by the header. Date#getTime keeps the result independent of the
 * computer's local timezone.
 */
export function getNitroOrbsClaim(
  nextRewardDate: string | null | undefined,
  now: Date,
): NitroOrbsClaim | null {
  if (!nextRewardDate || !Number.isFinite(now.getTime())) return null

  const rewardTime = Date.parse(nextRewardDate)
  if (!Number.isFinite(rewardTime)) return null

  const millisecondsUntilReward = rewardTime - now.getTime()
  if (millisecondsUntilReward <= 0) return null

  const minute = 1000 * 60
  const hour = minute * 60
  const day = hour * 24

  if (millisecondsUntilReward < 12 * hour) {
    return {
      value: Math.max(1, Math.ceil(millisecondsUntilReward / minute)),
      unit: 'minutes',
    }
  }

  if (millisecondsUntilReward < 48 * hour) {
    return {
      value: Math.max(1, Math.ceil(millisecondsUntilReward / hour)),
      unit: 'hours',
    }
  }

  return {
    value: Math.max(1, Math.ceil(millisecondsUntilReward / day)),
    unit: 'days',
  }
}
