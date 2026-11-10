import {
  createAssistantMessageEventStream,
  type Api,
  type AssistantMessageEvent,
  type AssistantMessageEventStream,
  type Model,
  type SimpleStreamOptions,
} from '@earendil-works/pi-ai';
import type {Provider} from '@earendil-works/pi-ai/models';

export const START_TIMEOUT_MS = 90_000;
export const POLICY = 1;
export type Observation = {
  model: string;
  outcome: 'started' | 'timeout' | 'cancelled' | 'ended';
  elapsedMs: number;
};
export interface GuardOptions {
  /** Test seam; the extension uses the fixed production policy. */
  timeoutMs?: number;
  observe?: (observation: Observation) => void;
}

/** Only the official endpoint; custom base URLs and gateways keep stock behavior. */
function eligible(model: Model<Api>): boolean {
  try {
    const url = new URL(model.baseUrl);
    return (
      url.origin === 'https://api.anthropic.com' &&
      url.pathname === '/' &&
      url.search === ''
    );
  } catch {
    return false;
  }
}

function isMessageStart(event: unknown): boolean {
  return (
    typeof event === 'object' &&
    event !== null &&
    'type' in event &&
    event.type === 'message_start'
  );
}

/**
 * Decorate the stock provider's simple streaming with a deadline from response
 * headers to the first raw `message_start`. The deadline aborts a request-private
 * signal, and the resulting aborted terminal is reported as a retryable timeout.
 * Auth, catalog, transport and retries stay upstream-owned.
 */
export function guardStreamSimple(
  stock: Provider,
  config: GuardOptions = {},
): Provider['streamSimple'] {
  const timeoutMs = config.timeoutMs ?? START_TIMEOUT_MS;
  if (!Number.isSafeInteger(timeoutMs) || timeoutMs <= 0)
    throw new Error('Anthropic startup timeout must be a positive integer');
  const emit = (observation: Observation) => {
    try {
      config.observe?.(observation);
    } catch {
      /* Diagnostics never change a provider result. */
    }
  };

  return (model, context, options) => {
    if (!eligible(model)) return stock.streamSimple(model, context, options);
    const caller = options?.signal;
    const own = new AbortController();
    let timer: ReturnType<typeof setTimeout> | undefined;
    let since: number | undefined;
    let started = false;
    let timedOut = false;
    const elapsed = () =>
      Math.max(0, Math.round(performance.now() - (since ?? 0)));
    // Ends the request's transport on every terminal path, including a parser
    // failure after message_start that would otherwise leave the body open.
    const close = () => {
      clearTimeout(timer);
      own.abort(new Error('Anthropic request finished'));
    };
    const guarded: SimpleStreamOptions = {
      ...options,
      signal: caller ? AbortSignal.any([caller, own.signal]) : own.signal,
      onResponse: async (response, responseModel) => {
        if (
          since === undefined &&
          /^text\/event-stream(?:;|$)/i.test(
            response.headers['content-type'] ?? '',
          )
        ) {
          since = performance.now();
          timer = setTimeout(() => {
            if (caller?.aborted) return;
            timedOut = true;
            own.abort(new Error('Anthropic startup deadline'));
          }, timeoutMs);
        }
        await options?.onResponse?.(response, responseModel);
      },
      onProviderStreamEvent: async (event, eventModel) => {
        if (!started && since !== undefined && isMessageStart(event)) {
          started = true;
          clearTimeout(timer);
          emit({model: model.id, outcome: 'started', elapsedMs: elapsed()});
        }
        await options?.onProviderStreamEvent?.(event, eventModel);
      },
    };
    let inner: AssistantMessageEventStream;
    try {
      inner = stock.streamSimple(model, context, guarded);
    } catch (error) {
      close();
      throw error;
    }
    const outer = createAssistantMessageEventStream();
    // Rewrites only this guard's own deadline; a caller abort that lands after
    // the deadline still ends as aborted.
    const finish = (event: AssistantMessageEvent): AssistantMessageEvent => {
      close();
      const timeout = event.type === 'error' && timedOut && !caller?.aborted;
      if (since !== undefined && !started)
        emit({
          model: model.id,
          outcome: timeout
            ? 'timeout'
            : caller?.aborted
              ? 'cancelled'
              : 'ended',
          elapsedMs: elapsed(),
        });
      if (!timeout) return event;
      return {
        type: 'error',
        reason: 'error',
        error: {
          ...event.error,
          stopReason: 'error',
          errorMessage: `Anthropic response timed out: no message_start within ${timeoutMs}ms after headers`,
        },
      };
    };
    const forward = async () => {
      let ended = false;
      try {
        for await (const event of inner) {
          const last = event.type === 'done' || event.type === 'error';
          ended ||= last;
          outer.push(last ? finish(event) : event);
        }
        outer.end(ended ? undefined : await inner.result());
      } finally {
        close();
      }
    };
    forward().catch((error: unknown) => {
      // Forwarding fails only on a defect; surface it as an uncaught error.
      queueMicrotask(() => {
        throw error;
      });
    });
    return outer;
  };
}
