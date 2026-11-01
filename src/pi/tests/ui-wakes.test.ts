import assert from 'node:assert/strict';
import test from 'node:test';
import {initTheme} from '@earendil-works/pi-coding-agent';
import {stripTerminalSequences, visibleWidth} from '@earendil-works/pi-tui';
import {getThemeByName} from '../../../node_modules/@earendil-works/pi-coding-agent/dist/modes/interactive/theme/theme.js';
import {
  createWakeWidget,
  orderWakes,
  wakeActivityBody,
  type WakeActivity,
} from '../extensions/wake/activity.ts';
import {createWidget} from '../extensions/workers/present.ts';
import {FooterView} from '../src/features/frame.ts';

initTheme('dark', false);
const fixture: WakeActivity = {
  id: 'k-fixture',
  text: 'Inspect verification',
  at: 120000,
  fired: 1,
  limit: 5,
  every: 120000,
  check: 'fixture-check',
  on: 'change',
  state: 'scheduled',
};

test('wakes distinguish scheduled, checking and queued; bounded overview retains full inspection', () => {
  const rows: WakeActivity[] = [
    fixture,
    {
      ...fixture,
      id: 'k-check',
      text: 'Check status',
      state: 'checking',
      lastCode: 1,
    },
    {...fixture, id: 'k-queued', text: 'Follow-up queued', state: 'queued'},
  ];
  assert.deepEqual(
    orderWakes(rows).map(row => row.id),
    ['k-check', 'k-queued', 'k-fixture'],
  );
  for (const name of ['dark', 'light']) {
    const theme = getThemeByName(name)!;
    const widget = createWakeWidget(
      () => rows,
      () => 0,
      theme,
    );
    const plain = stripTerminalSequences(widget.render(120).join('\n'));
    assert.match(plain, /◷ Wakes.*\/wakes/);
    assert.match(plain, /checking/);
    assert.match(plain, /queued/);
    assert.match(plain, /exit 1/);
    assert.match(plain, /\+1 more/);
    assert.equal(widget.render(120).length, 4);
    const full = stripTerminalSequences(
      wakeActivityBody(rows, 0, theme, true).render(100).join('\n'),
    );
    assert.match(full, /in 2m/);
    assert.match(full, /Check \(change\): fixture-check/);
    assert.match(full, /Last exit: not observed/);
    assert.match(full, /k-fixture/);
    for (let width = 1; width < 161; width++) {
      assert.ok(
        widget.render(width).every(line => visibleWidth(line) <= width),
      );
    }
    assert.deepEqual(
      createWakeWidget(
        () => [],
        () => 0,
        theme,
      ).render(100),
      [],
    );
    assert.match(
      stripTerminalSequences(
        wakeActivityBody([], 0, theme, true).render(100).join('\n'),
      ),
      /No active wakes/,
    );
    const unsafe = createWakeWidget(
      () => [{...fixture, text: '日本語 👩🏽‍💻\x1b[2J status'}],
      () => 0,
      theme,
    );
    assert.doesNotMatch(unsafe.render(100).join('\n'), /\x1b\[2J/);
  }
});

test('worker runtime metadata aligns with the primary session across models, efforts, themes and resize', () => {
  for (const name of ['dark', 'light']) {
    const theme = getThemeByName(name)!;
    const footer = new FooterView();
    footer.update(
      {
        project: 'example',
        model: 'primary-model',
        thinking: 'high',
        contextPercent: 50,
        statuses: [],
      },
      theme,
    );
    const rows = ['high', 'medium'].map((thinking, i) => ({
      label: `worker-${i}`,
      model: `example/${i ? 'other-model' : 'primary-model'}`,
      thinking,
      state: 'running',
      context: 50,
      contextBasis: i ? undefined : ('compact' as const),
      depth: i,
      elapsed: '2m',
      calls: 1,
      tokens: 1,
      detail: 'read src/report.ts',
      idleMs: 0,
    }));
    const widget = createWidget(() => rows, theme);
    for (const width of [120, 160, 220]) {
      const primary = stripTerminalSequences(footer.render(width)[0]!);
      const workers = widget
        .render(width)
        .map(stripTerminalSequences)
        .filter(line => line.includes('worker-'));
      for (const worker of workers) {
        for (const anchor of ['✦', '━━━━━─────']) {
          const column = (text: string) =>
            visibleWidth(text.slice(0, text.indexOf(anchor)));
          assert.ok(worker.includes(anchor));
          assert.equal(
            column(worker),
            column(primary),
            `${anchor} at ${width}`,
          );
        }
      }
    }
    for (let width = 1; width <= 160; width++)
      assert.ok(
        widget.render(width).every(line => visibleWidth(line) <= width),
      );
  }
});
