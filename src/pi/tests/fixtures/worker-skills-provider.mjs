// Deterministic, network-free child provider for the skill handoff host test:
// it never calls a tool, and reports the skill blocks in its effective system
// prompt, every skill-body marker there, its cwd, and the tool calls and
// assistant turns already in its context.
import {
  createAssistantMessageEventStream,
  getCurrentSystemPrompt,
} from '@earendil-works/pi-ai';

export const fixtureModel = {
  provider: 'worker-skills-fixture',
  id: 'fixture',
  name: 'Skills fixture',
  api: 'worker-skills-fixture',
  baseUrl: 'https://example.invalid',
  reasoning: false,
  input: ['text'],
  contextWindow: 100_000,
  maxTokens: 1000,
  cost: {input: 0, output: 0, cacheRead: 0, cacheWrite: 0},
};

const usage = {
  input: 0,
  output: 0,
  cacheRead: 0,
  cacheWrite: 0,
  totalTokens: 0,
  cost: {input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0},
};

export default function register(pi) {
  pi.registerProvider(fixtureModel.provider, {
    api: fixtureModel.api,
    apiKey: 'fixture',
    baseUrl: fixtureModel.baseUrl,
    models: [fixtureModel],
    streamSimple(model, context) {
      const stream = createAssistantMessageEventStream();
      const prompt = getCurrentSystemPrompt(context.messages);
      const skills = [
        ...prompt.matchAll(
          /<skill name="([^"]+)" location="([^"]+)">\n([\s\S]*?)\n<\/skill>/g,
        ),
      ].map(([, name, location, content]) => ({name, location, content}));
      const assistant = context.messages.filter(
        entry => entry.role === 'assistant',
      );
      const message = {
        role: 'assistant',
        api: model.api,
        provider: model.provider,
        model: model.id,
        timestamp: Date.now(),
        content: [
          {
            type: 'text',
            text: JSON.stringify({
              skills,
              bodies: prompt.match(/BODY-[a-z]+/g) ?? [],
              cwd: process.cwd(),
              toolCalls: assistant.flatMap(entry =>
                entry.content.flatMap(block =>
                  block.type === 'toolCall' ? [block.name] : [],
                ),
              ),
              priorTurns: assistant.length,
            }),
          },
        ],
        stopReason: 'stop',
        usage,
      };
      queueMicrotask(() => {
        stream.push({type: 'start', partial: message});
        stream.push({type: 'done', reason: 'stop', message});
        stream.end();
      });
      return stream;
    },
  });
}
