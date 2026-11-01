import {AsyncLocalStorage} from 'node:async_hooks';
import type {
  AssistantMessageEventStream,
  StreamOptions,
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

/** Node's fetch input; the DOM lib that names it RequestInfo is not part of this runtime. */
type FetchInput = Parameters<typeof fetch>[0];

function eligible(input: FetchInput, init?: RequestInit): boolean {
  try {
    const url = new URL(input instanceof Request ? input.url : String(input));
    return (
      url.origin === 'https://api.anthropic.com' &&
      url.pathname === '/v1/messages' &&
      (
        init?.method ?? (input instanceof Request ? input.method : 'GET')
      ).toUpperCase() === 'POST'
    );
  } catch {
    return false;
  }
}

/** Decorate only the stock provider's streaming calls; auth/catalog remain upstream-owned. */
export function guardProvider(
  stock: Provider,
  config: GuardOptions = {},
): Provider {
  const timeoutMs = config.timeoutMs ?? START_TIMEOUT_MS;
  if (!Number.isSafeInteger(timeoutMs) || timeoutMs <= 0)
    throw new Error('Anthropic startup timeout must be a positive integer');

  // Pi keys its federation client/token cache by fetch identity. Keep that identity
  // stable while routing each async request to its own cancellation/deadline state.
  const requests = new AsyncLocalStorage<typeof fetch>();
  const fetches = new WeakMap<typeof fetch, typeof fetch>();
  function stableFetch(underlying: typeof fetch): typeof fetch {
    let wrapped = fetches.get(underlying);
    if (!wrapped) {
      wrapped = (input, init) =>
        (requests.getStore() ?? underlying)(input, init);
      fetches.set(underlying, wrapped);
    }
    return wrapped;
  }

  function run<O extends StreamOptions>(
    model: string,
    options: O | undefined,
    launch: (options: O) => AssistantMessageEventStream,
  ) {
    const releases = new Set<() => void>();
    let started: (() => void) | undefined;
    const emit = (observation: Observation) => {
      try {
        config.observe?.(observation);
      } catch {
        /* Diagnostics never change a provider result. */
      }
    };
    const fetchImpl = options?.fetch ?? globalThis.fetch;
    const wrapped = {
      ...options,
      onProviderStreamEvent: async (
        event: unknown,
        physical: Parameters<
          NonNullable<StreamOptions['onProviderStreamEvent']>
        >[1],
      ) => {
        if (
          typeof event === 'object' &&
          event !== null &&
          'type' in event &&
          event.type === 'message_start'
        )
          started?.();
        await options?.onProviderStreamEvent?.(event, physical);
      },
      fetch: async (
        input: FetchInput,
        init?: RequestInit,
      ): Promise<Response> => {
        if (!eligible(input, init)) return fetchImpl(input, init);
        const controller = new AbortController();
        const caller =
          init?.signal ?? (input instanceof Request ? input.signal : undefined);
        const signal = caller
          ? AbortSignal.any([caller, controller.signal])
          : controller.signal;
        const response = await fetchImpl(input, {...init, signal});
        if (
          !response.ok ||
          !response.body ||
          !/^text\/event-stream(?:;|$)/i.test(
            response.headers.get('content-type') ?? '',
          )
        )
          return response;
        const reader = response.body.getReader();
        const since = performance.now();
        let timer: ReturnType<typeof setTimeout> | undefined;
        let closed = false;
        let reported = false;
        let output: ReadableStreamDefaultController<Uint8Array>;
        const clear = () => {
          clearTimeout(timer);
          timer = undefined;
        };
        const report = (outcome: Observation['outcome']) => {
          if (reported) return;
          reported = true;
          emit({
            model,
            outcome,
            elapsedMs: Math.max(0, Math.round(performance.now() - since)),
          });
        };
        const cleanup = () => {
          clear();
          signal.removeEventListener('abort', aborted);
          releases.delete(release);
          if (started === markStarted) started = undefined;
        };
        const stop = (reason: unknown, outcome: Observation['outcome']) => {
          if (closed) return;
          closed = true;
          report(outcome);
          cleanup();
          output.error(reason);
          // Error the consumer immediately; cancellation must not await a broken transport.
          void reader
            .cancel(reason)
            .catch(() => {})
            .finally(() => reader.releaseLock());
          controller.abort(reason);
        };
        const aborted = () => stop(signal.reason, 'cancelled');
        const release = () =>
          stop(new Error('Anthropic response consumer finished'), 'ended');
        const markStarted = () => {
          clear();
          report('started');
        };
        started = markStarted;
        const body = new ReadableStream<Uint8Array>({
          start(c) {
            output = c;
            releases.add(release);
            signal.addEventListener('abort', aborted, {once: true});
            timer = setTimeout(() => {
              if (caller?.aborted) {
                aborted();
                return;
              }
              stop(
                new Error(
                  `Anthropic response timed out: no message_start within ${timeoutMs}ms after headers`,
                ),
                'timeout',
              );
            }, timeoutMs);
            if (signal.aborted) aborted();
          },
          async pull(c) {
            try {
              const next = await reader.read();
              if (closed) return;
              if (next.done) {
                closed = true;
                report('ended');
                cleanup();
                reader.releaseLock();
                c.close();
              } else c.enqueue(next.value);
            } catch (error) {
              stop(error, caller?.aborted ? 'cancelled' : 'ended');
            }
          },
          cancel(reason) {
            stop(
              reason ?? new Error('Anthropic response cancelled'),
              'cancelled',
            );
          },
        });
        return new Response(body, {
          status: response.status,
          statusText: response.statusText,
          headers: response.headers,
        });
      },
    } as O;
    const requestFetch = wrapped.fetch!;
    wrapped.fetch = stableFetch(fetchImpl);
    const finish = () => {
      for (const release of [...releases]) release();
    };
    try {
      const stream = requests.run(requestFetch, () => launch(wrapped));
      // Pi's parser can finish on an error without cancelling its response reader.
      // The public result promise closes that gap without consuming/rewriting events.
      void stream.result().then(finish, finish);
      return stream;
    } catch (error) {
      finish();
      throw error;
    }
  }

  return {
    ...stock,
    stream: (model, context, options) =>
      run(model.id, options, wrapped => stock.stream(model, context, wrapped)),
    streamSimple: (model, context, options) =>
      run(model.id, options, wrapped =>
        stock.streamSimple(model, context, wrapped),
      ),
  };
}
