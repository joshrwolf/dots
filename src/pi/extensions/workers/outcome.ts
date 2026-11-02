import {fields} from './protocol.ts';

// A child run is identified by its assistant message_end, not by the RPC
// last-text query (which may belong to an earlier run or a partial response).
export interface AssistantResponse {
  stopReason: string;
  text: string;
  error?: string;
  truncated?: true;
}

/** What an owner reads from one assistant message: its outcome, usage, and reasoning headings' source. */
export interface AssistantReading extends AssistantResponse {
  /** Total tokens the response reported, when positive. */
  tokens?: number;
  /** Pi's best cost total for this response, in dollars. */
  cost?: number;
  /** Set when that total is the provider's billed cost rather than a catalog estimate. */
  costBilled?: true;
  /** Completed reasoning blocks, in order. */
  thinking: string[];
}

export function assistantResponse(
  message: unknown,
): AssistantReading | undefined {
  const record = fields(message);
  if (record?.role !== 'assistant' || typeof record.stopReason !== 'string')
    return undefined;
  const parts = Array.isArray(record.content) ? record.content.map(fields) : [];
  const usage = fields(record.usage);
  const tokens = usage?.totalTokens;
  const costs = fields(usage?.cost);
  const cost = costs?.total;
  return {
    stopReason: record.stopReason,
    text: parts
      .flatMap(part =>
        part?.type === 'text' && typeof part.text === 'string'
          ? [part.text]
          : [],
      )
      .join(''),
    thinking: parts.flatMap(part =>
      part?.type === 'thinking' && typeof part.thinking === 'string'
        ? [part.thinking]
        : [],
    ),
    ...(typeof record.errorMessage === 'string'
      ? {error: record.errorMessage}
      : {}),
    ...(typeof tokens === 'number' && Number.isSafeInteger(tokens) && tokens > 0
      ? {tokens}
      : {}),
    ...(typeof cost === 'number' && Number.isFinite(cost) && cost >= 0
      ? {cost, ...(costs?.totalFromProvider === true ? {costBilled: true} : {})}
      : {}),
  };
}

export type SettledOutcome =
  | {state: 'completed'; text: string}
  | {state: 'interrupted'; text: string}
  | {state: 'failed' | 'unknown'; text: string};

export function settledOutcome(
  response: AssistantResponse | undefined,
  aborted: boolean,
): SettledOutcome {
  if (
    aborted ||
    response?.stopReason === 'aborted' ||
    response?.stopReason === 'length' ||
    response?.stopReason === 'deferred'
  ) {
    return {state: 'interrupted', text: response?.text ?? ''};
  }
  if (response?.stopReason === 'error') {
    return {
      state: 'failed',
      text: response.error ?? 'Provider response failed',
    };
  }
  if (response?.stopReason === 'stop')
    return {state: 'completed', text: response.text};
  return {
    state: 'unknown',
    text: response
      ? `Last assistant response ended with ${response.stopReason}; no final answer was confirmed`
      : 'No assistant response was recorded for this run',
  };
}

/** A worker's running cost total; `costBilled` absent means an estimate. */
export interface CostTotal {
  cost?: number | undefined;
  costBilled?: true | undefined;
}

/** Add one response's cost. The total stays provider-billed only while every part is. */
export function addCost(
  total: CostTotal,
  response: Pick<AssistantReading, 'cost' | 'costBilled'>,
): CostTotal {
  if (response.cost === undefined) return total;
  const billed =
    (total.cost === undefined || total.costBilled) && response.costBilled;
  return {
    cost: (total.cost ?? 0) + response.cost,
    costBilled: billed ? true : undefined,
  };
}
