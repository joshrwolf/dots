import {appendFileSync, readFileSync} from 'node:fs';
import {join} from 'node:path';
import {getAgentDir} from '@earendil-works/pi-coding-agent';
import guard from '../../extensions/anthropic-request-guard/index.ts';
import workers from '../../extensions/workers/index.ts';
import type {
  ExtensionAPI,
  ExtensionCommandContext,
  ExtensionToolContext,
  ProviderConfig,
  ToolDefinition,
} from '@earendil-works/pi-coding-agent';

/** A tool context for a command-driven call, built as Pi builds its own: the command context's lazy getters, plus tool reach. */
function toolContext(ctx: ExtensionCommandContext): ExtensionToolContext {
  const context = Object.defineProperties(
    {},
    Object.getOwnPropertyDescriptors(ctx),
  ) as ExtensionCommandContext;
  return Object.assign(context, {
    tools: [],
    executeTool: () =>
      Promise.reject(new Error('fixture tools do not call other tools')),
  });
}

/** Loaded only by isolated installed-host acceptance. No external network route. */
export default function (pi: ExtensionAPI): void {
  const config = JSON.parse(
    readFileSync(join(getAgentDir(), 'guard-fixture.json'), 'utf8'),
  );
  const url = new URL(config.url);
  if (url.hostname !== '127.0.0.1')
    throw new Error('fixture requires loopback HTTP');
  const mark = (record: object) =>
    appendFileSync(
      config.marker,
      JSON.stringify({
        pid: process.pid,
        worker: process.env.DOTS_WORKER_ID,
        ...record,
      }) + '\n',
    );
  const tools = new Map<string, ToolDefinition>();
  const proxy = new Proxy(pi, {
    get(target, key) {
      if (key === 'registerProvider')
        return (name: string, provider: ProviderConfig) => {
          if (name !== 'anthropic' || !provider.streamSimple)
            throw new Error('fixture expects Anthropic stream override');
          const streamSimple = provider.streamSimple;
          mark({type: 'guard_registered'});
          const fetchLocal: typeof fetch = (_input, init) =>
            fetch(url, {
              ...init,
              headers: {
                ...Object.fromEntries(new Headers(init?.headers)),
                'x-fixture-worker': process.env.DOTS_WORKER_ID ?? 'parent',
              },
            });
          target.registerProvider(name, {
            ...provider,
            streamSimple: (m, c, o) =>
              streamSimple(m, c, {...o, fetch: fetchLocal}),
          });
        };
      if (key === 'registerTool')
        return (tool: ToolDefinition) => {
          tools.set(tool.name, tool);
          target.registerTool(tool);
        };
      if (key === 'sendMessage')
        return (message: unknown, options: object) => {
          mark({type: 'owner_message', message, options});
          // Observe delivery without spending a parent model turn responding to it.
          return target.sendMessage(
            message as Parameters<ExtensionAPI['sendMessage']>[0],
            {...options, triggerTurn: false},
          );
        };
      return Reflect.get(target, key);
    },
  });
  guard(proxy, {timeoutMs: 100});
  workers(proxy, {
    command: [
      'pi',
      '--offline',
      '--no-mcp',
      '--no-context-files',
      '--no-skills',
      '--no-prompt-templates',
      '--no-themes',
      '--no-approve',
    ],
  });
  pi.on('agent_settled', event =>
    mark({type: 'settled', aborted: event.aborted}),
  );
  pi.on('session_shutdown', () => mark({type: 'shutdown'}));
  pi.on('session_compact', () => mark({type: 'compacted'}));
  pi.registerCommand('fixture-compact', {
    description: 'isolated compaction driver',
    async handler(_args, ctx) {
      await new Promise<void>((resolve, reject) =>
        ctx.compact({onComplete: () => resolve(), onError: reject}),
      );
    },
  });
  pi.registerCommand('fixture-call', {
    description: 'isolated worker tool driver',
    async handler(args, ctx) {
      const {id, name, params} = JSON.parse(args);
      try {
        const tool = tools.get(name);
        if (!tool) throw new Error('fixture tool missing');
        const result = await tool.execute(
          id,
          params,
          undefined,
          undefined,
          toolContext(ctx),
        );
        mark({type: 'tool_result', id, result});
      } catch (error) {
        mark({type: 'tool_error', id, error: String(error)});
      }
    },
  });
}
