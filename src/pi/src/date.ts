/** Nonnegative integer timestamps that JavaScript can render, in explicit wire units. */
export function isTimestamp(
  value: unknown,
  unit: 'seconds' | 'milliseconds',
): value is number {
  return (
    typeof value === 'number' &&
    Number.isSafeInteger(value) &&
    value >= 0 &&
    value <= (unit === 'seconds' ? 8_640_000_000_000 : 8_640_000_000_000_000)
  );
}
