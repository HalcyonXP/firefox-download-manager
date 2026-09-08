export const PROTOCOL_VERSION = 2 as const;
export const MAX_MESSAGE_BYTES = 1_048_576 as const;

const CORRELATION_ID_PATTERN = /^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$/u;

export function isValidCorrelationId(value: string): boolean {
  return CORRELATION_ID_PATTERN.test(value);
}
