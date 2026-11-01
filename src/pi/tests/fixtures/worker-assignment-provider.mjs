// Deterministic, network-free provider for the real Pi child lifecycle test.
import path from 'node:path';
import {
  createAssistantMessageEventStream,
  getCurrentSystemPrompt,
} from '@earendil-works/pi-ai';

export const fixtureModel = {
  provider: 'worker-assignment-fixture',
  id: 'fixture',
  name: 'Assignment fixture',
  api: 'worker-assignment-fixture',
  baseUrl: 'https://example.invalid',
  reasoning: false,
  input: ['text'],
  contextWindow: 100_000,
  maxTokens: 1000,
  cost: {input: 0, output: 0, cacheRead: 0, cacheWrite: 0},
};
export default function register(pi) {
  pi.registerProvider(fixtureModel.provider, {
    api: fixtureModel.api,
    apiKey: 'fixture',
    baseUrl: fixtureModel.baseUrl,
    models: [fixtureModel],
    streamSimple(model, context) {
      const stream = createAssistantMessageEventStream();
      const text =
        getCurrentSystemPrompt(context.messages)
          .split('\n')
          .find(line => line.startsWith('Task association:')) ??
        'No task association';
      const message = {
        role: 'assistant',
        api: model.api,
        provider: model.provider,
        model: model.id,
        timestamp: Date.now(),
        content: [{type: 'text', text}],
        stopReason: 'stop',
        usage: {
          input: 0,
          output: 0,
          cacheRead: 0,
          cacheWrite: 0,
          totalTokens: 0,
          cost: {input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0},
        },
      };
      if (
        !context.messages.some(
          entry => entry.role === 'toolResult' && entry.toolName === 'write',
        )
      ) {
        message.content = [
          {
            type: 'toolCall',
            id: 'fixture-write',
            name: 'write',
            arguments: {
              path: 'result.txt',
              content: `Fixture artifact from ${path.basename(process.cwd())}\n`,
            },
          },
        ];
        message.stopReason = 'toolUse';
      }
      queueMicrotask(() => {
        stream.push({type: 'start', partial: message});
        stream.push({type: 'done', reason: message.stopReason, message});
        stream.end();
      });
      return stream;
    },
  });
}
