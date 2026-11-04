import assert from 'node:assert/strict';
import test from 'node:test';
import {
  AssistantMessageComponent,
  initTheme,
  type ExtensionAPI,
  type ExtensionContext,
  type MarkdownTransformContext,
  type MarkdownTransformer,
} from '@earendil-works/pi-coding-agent';
import {stripTerminalSequences, visibleWidth} from '@earendil-works/pi-tui';
import {thinkingFixture} from '../dev/thinking.ts';
import register from '../extensions/ui/index.ts';
import {
  hiddenThinkingLabel,
  thinkingMarkdown,
} from '../src/features/thinking.ts';
import {displayLimits} from '@joshrwolf/pi-kit/ui';

const context: MarkdownTransformContext = {
  messageType: 'assistant-thinking',
  isStreaming: false,
  availableWidth: 80,
};
const plain = (view: AssistantMessageComponent, width = 120) =>
  stripTerminalSequences(view.render(width).join('\n'));

function registration() {
  const transforms: MarkdownTransformer[] = [];
  const handlers = new Map<
    string,
    (event: unknown, ctx: ExtensionContext) => void
  >();
  const labels: (string | undefined)[] = [];
  const ctx = {
    mode: 'tui',
    hasUI: true,
    ui: {
      setHiddenThinkingLabel(label?: string) {
        labels.push(label);
      },
      setStatus() {},
      setFooter() {},
      getEditorComponent() {
        return undefined;
      },
      setEditorComponent() {},
    },
  } as unknown as ExtensionContext;
  register({
    registerToolRenderer() {},
    registerMarkdownTransformer(transform: MarkdownTransformer) {
      transforms.push(transform);
    },
    on(name: string, handler: (event: unknown, ctx: ExtensionContext) => void) {
      handlers.set(name, handler);
    },
  } as unknown as ExtensionAPI);
  return {
    transforms,
    labels,
    start(mode = 'tui') {
      handlers.get('session_start')?.({}, {...ctx, mode} as ExtensionContext);
    },
    dispose() {
      handlers.get('session_shutdown')?.({}, ctx);
    },
  };
}

test('thinking summaries get one compact label; ordinary assistant and user Markdown stay untouched', () => {
  for (const text of [
    'Reviewing dependencies',
    '**Reviewing dependencies**',
    '__Reviewing dependencies__',
    '## Reviewing dependencies',
    '## Reviewing dependencies ##',
  ]) {
    for (const isStreaming of [false, true])
      assert.equal(
        thinkingMarkdown(text, {...context, isStreaming}),
        '**Thinking** · Reviewing dependencies',
      );
    for (const messageType of ['user', 'assistant'] as const)
      assert.equal(thinkingMarkdown(text, {...context, messageType}), text);
  }
  assert.equal(thinkingMarkdown('  \n', context), '  \n');
  assert.equal(
    thinkingMarkdown('**First** and **second**', context),
    '**Thinking** · **First** and **second**',
  );
});

test('structured and unfinished thoughts preserve Markdown inside a native quote, with conservative budget fallback', () => {
  for (const text of [
    '- Check the runtime',
    '```ts',
    '> A quotation',
    '1. Review',
    '---',
    '## Plan\n\n- Keep `code`\n\n```ts\n  const value = 1;\n```\n\n[notes](https://example.com)',
    'Paragraph\n\n```ts\nconst value =',
    'x'.repeat(241),
  ]) {
    const transformed = thinkingMarkdown(text, context);
    assert.ok(transformed.startsWith('**Thinking**\n\n'));
    assert.equal(
      transformed
        .slice('**Thinking**\n\n'.length)
        .split('\n')
        .map(line => line.slice(2))
        .join('\n'),
      text,
    );
  }
  for (const text of [
    'x'.repeat(displayLimits.sourceCodeUnits + 1),
    'x\n'.repeat(displayLimits.sourceLines + 1),
  ])
    assert.equal(thinkingMarkdown(text, context), text);
});

test('registered transformer uses real assistant rendering, native disclosure, themes and streaming without changing records', () => {
  const r = registration();
  try {
    assert.deepEqual(r.transforms, [thinkingMarkdown]);
    for (const name of ['dark', 'light']) {
      initTheme(name, false);
      for (let phase = 0; phase < 3; phase++) {
        const message = thinkingFixture(phase);
        const before = structuredClone(message);
        const view = new AssistantMessageComponent(
          undefined,
          false,
          undefined,
          hiddenThinkingLabel,
          0,
          r.transforms,
        );
        view.updateContent(message, phase === 2);
        assert.match(plain(view), /Thinking/);
        if (phase === 0)
          assert.match(plain(view), /Thinking · Reviewing dependency updates/);
        if (phase === 1)
          assert.match(
            plain(view),
            /Decision: run the focused checks before updating/,
          );
        if (phase === 2) assert.match(plain(view), /const version =/);
        for (const width of [20, 40, 80, 120, 240, 400])
          assert.ok(
            view.render(width).every(line => visibleWidth(line) <= width),
            `${name}/${phase}/${width}`,
          );
        view.setHideThinkingBlock(true);
        assert.match(plain(view), /▸ Thinking/);
        assert.doesNotMatch(
          plain(view),
          /Reviewing dependency|Compare versions|const version/,
        );
        view.setHideThinkingBlock(false);
        assert.match(plain(view), /Thinking/);
        assert.deepEqual(message, before);
      }
    }
    const message = thinkingFixture();
    message.content.push(
      {type: 'text', text: 'I will run the checks.'},
      {type: 'thinking', thinking: 'Checking the outcome'},
    );
    const view = new AssistantMessageComponent(
      message,
      false,
      undefined,
      hiddenThinkingLabel,
      0,
      r.transforms,
    );
    assert.equal(plain(view).match(/Thinking ·/g)?.length, 2);
    assert.match(plain(view), /I will run the checks/);
  } finally {
    r.dispose();
  }
});

test('compact thinking disclosure is TUI-only and shutdown does not clobber later owners', () => {
  const r = registration();
  for (const mode of ['rpc', 'print', 'json']) r.start(mode);
  assert.deepEqual(r.labels, []);
  r.start();
  assert.deepEqual(r.labels, [hiddenThinkingLabel]);
  assert.equal(r.transforms.length, 1);
  r.dispose();
  assert.deepEqual(r.labels, [hiddenThinkingLabel]);
});
