import assert from 'node:assert/strict';
import {
  appendFileSync,
  mkdtempSync,
  readFileSync,
  renameSync,
  rmSync,
  statSync,
  truncateSync,
  writeFileSync,
} from 'node:fs';
import {tmpdir} from 'node:os';
import path from 'node:path';
import test from 'node:test';
import {initTheme, type SessionEntry} from '@earendil-works/pi-coding-agent';
import {stripTerminalSequences, type TUI} from '@earendil-works/pi-tui';
import {SessionFixture, assistantMessage} from '../dev/sessions.ts';
import {SessionFollower} from '../extensions/workers/follow.ts';
import {
  TranscriptProjection,
  projectTranscript,
} from '../extensions/workers/projection.ts';
import {WorkerTranscript} from '../extensions/workers/transcript.ts';

initTheme('dark', false);
const plain = (lines: string[]) => stripTerminalSequences(lines.join('\n'));
const options = {
  outputPad: 1,
  hideThinkingBlock: false,
  showImages: false,
  imageWidthCells: 60,
};

function scratch(t: test.TestContext): string {
  const directory = mkdtempSync(path.join(tmpdir(), 'dots-worker-view-'));
  t.after(() => rmSync(directory, {recursive: true, force: true}));
  return directory;
}

const texts = (follower: SessionFollower) =>
  follower.session.entries.map(entry =>
    entry.type === 'message' && entry.message.role === 'user'
      ? entry.message.content
      : entry.type,
  );

test('follower reads only complete lines and resumes a partial record', t => {
  const file = path.join(scratch(t), 'session.jsonl');
  const follower = new SessionFollower(file);
  assert.equal(follower.poll(), true);
  assert.match(follower.session.notice!, /not saved yet/);
  assert.equal(follower.poll(), false);

  const session = new SessionFixture(file, '/fixture');
  session.user('first');
  const full = readFileSync(file, 'utf8');
  const before = statSync(file);
  // A record the writer has not finished is left for the next poll.
  const partial = JSON.stringify({
    type: 'message',
    id: 'partial-1',
    parentId: '00000001',
    timestamp: '2026-01-01T00:01:00.000Z',
    message: {role: 'user', content: 'second', timestamp: 1},
  });
  appendFileSync(file, partial.slice(0, 20));
  assert.equal(follower.poll(), true);
  assert.equal(follower.session.notice, undefined);
  assert.equal(follower.session.header?.cwd, '/fixture');
  assert.deepEqual(texts(follower), ['first']);
  appendFileSync(file, partial.slice(20) + '\n');
  assert.equal(follower.poll(), true);
  assert.deepEqual(texts(follower), ['first', 'second']);
  assert.equal(follower.poll(), false);
  // Reading never writes: the file grew only by what this test appended.
  assert.equal(
    readFileSync(file, 'utf8'),
    full + partial + '\n',
    'follower left the file as written',
  );
  assert.equal(statSync(file).ino, before.ino);
});

test('follower reloads a replaced, shrunk or rewritten file and reports a missing one', t => {
  const directory = scratch(t);
  const file = path.join(directory, 'session.jsonl');
  const first = new SessionFixture(file, '/fixture');
  first.user('one');
  first.user('two');
  const follower = new SessionFollower(file);
  follower.poll();
  assert.deepEqual(texts(follower), ['one', 'two']);

  // Replaced by rename: a different inode.
  const other = path.join(directory, 'other.jsonl');
  new SessionFixture(other, '/elsewhere').user('replacement');
  renameSync(other, file);
  assert.equal(follower.poll(), true);
  assert.deepEqual(texts(follower), ['replacement']);
  assert.equal(follower.session.header?.cwd, '/elsewhere');

  // Rewritten in place, shorter: the same inode.
  const lines = readFileSync(file, 'utf8').split('\n');
  truncateSync(file, 0);
  writeFileSync(file, lines[0] + '\n');
  assert.equal(follower.poll(), true);
  assert.deepEqual(texts(follower), []);

  // Rewritten in place, longer, with another header.
  writeFileSync(
    file,
    [
      JSON.stringify({
        type: 'session',
        id: 'rewritten',
        timestamp: '2026-01-01T00:00:00.000Z',
        cwd: '/rewritten',
      }),
      JSON.stringify({
        type: 'message',
        id: 'a',
        parentId: null,
        timestamp: '2026-01-01T00:00:01.000Z',
        message: {role: 'user', content: 'again', timestamp: 1},
      }),
      '',
    ].join('\n'),
  );
  assert.equal(follower.poll(), true);
  assert.deepEqual(texts(follower), ['again']);
  assert.equal(follower.session.header?.cwd, '/rewritten');

  rmSync(file);
  assert.equal(follower.poll(), true);
  assert.match(follower.session.notice!, /missing/);
  assert.deepEqual(texts(follower), []);
});

test('follower skips unreadable records and refuses a file without a session header', t => {
  const directory = scratch(t);
  const file = path.join(directory, 'session.jsonl');
  new SessionFixture(file, '/fixture').user('kept');
  appendFileSync(file, '{not json}\n{"type":"message"}\n');
  const follower = new SessionFollower(file);
  follower.poll();
  assert.deepEqual(texts(follower), ['kept']);
  assert.match(follower.session.notice!, /2 unreadable records skipped/);

  const foreign = path.join(directory, 'foreign.jsonl');
  writeFileSync(
    foreign,
    JSON.stringify({type: 'message', id: 'x', message: {role: 'user'}}) + '\n',
  );
  const other = new SessionFollower(foreign);
  other.poll();
  assert.deepEqual(other.session.entries, []);
  assert.match(other.session.notice!, /not a Pi session file/);
});

test('follower shows the active branch with compaction applied', t => {
  const file = path.join(scratch(t), 'session.jsonl');
  const header = {
    type: 'session',
    id: 's',
    timestamp: '2026-01-01T00:00:00.000Z',
    cwd: '/fixture',
  };
  const message = (id: string, parentId: string | null, content: string) => ({
    type: 'message',
    id,
    parentId,
    timestamp: '2026-01-01T00:00:01.000Z',
    message: {role: 'user', content, timestamp: 1},
  });
  writeFileSync(
    file,
    [
      header,
      message('a', null, 'summarized'),
      message('b', 'a', 'kept'),
      message('abandoned', 'b', 'abandoned branch'),
      {
        type: 'compaction',
        id: 'c',
        parentId: 'b',
        timestamp: '2026-01-01T00:00:02.000Z',
        summary: 'Earlier work summarized.',
        firstKeptEntryId: 'b',
        tokensBefore: 100,
      },
      message('d', 'c', 'after'),
      {
        type: 'session_info',
        id: 'e',
        parentId: 'd',
        timestamp: '2026-01-01T00:00:03.000Z',
        name: 'Named worker',
      },
    ]
      .map(line => JSON.stringify(line))
      .join('\n') + '\n',
  );
  const follower = new SessionFollower(file);
  follower.poll();
  assert.deepEqual(texts(follower), [
    'compaction',
    'kept',
    'after',
    'session_info',
  ]);
  const items = projectTranscript(follower.session.entries);
  assert.deepEqual(
    items.map(item => item.kind),
    ['compaction', 'user', 'user'],
  );
});

test('follower starts a large file at its read window and keeps following', t => {
  const file = path.join(scratch(t), 'session.jsonl');
  const session = new SessionFixture(file, '/fixture');
  for (let i = 0; i < 40; i++) session.user(`early ${i} ${'x'.repeat(200)}`);
  session.user('inside the window');
  const follower = new SessionFollower(file, 600);
  assert.equal(follower.poll(), true);
  assert.equal(follower.session.truncated, true);
  assert.equal(follower.session.header?.cwd, '/fixture');
  assert.equal(follower.session.notice, undefined);
  assert.deepEqual(texts(follower).slice(-1), ['inside the window']);
  assert.ok(texts(follower).length < 5, 'early records are not read');
  session.user('appended');
  assert.equal(follower.poll(), true);
  assert.deepEqual(texts(follower).slice(-2), [
    'inside the window',
    'appended',
  ]);
  assert.equal(follower.poll(), false);
  const small = new SessionFollower(file);
  small.poll();
  assert.equal(small.session.truncated, false);
  assert.equal(texts(small).length, 42);
});

test('projection pairs tool results, marks calls of failed responses and keeps skill invocations', () => {
  const entries = [
    {
      type: 'message',
      id: '1',
      parentId: null,
      timestamp: '2026-01-01T00:00:00.000Z',
      message: {
        role: 'user',
        content:
          '<skill name="fixture" location="/skills/fixture/SKILL.md">\nUse the fixture.\n</skill>\n\nPlease run it.',
        timestamp: 1,
      },
    },
    {
      type: 'message',
      id: '2',
      parentId: '1',
      timestamp: '2026-01-01T00:00:01.000Z',
      message: assistantMessage(
        [{type: 'toolCall', id: 'call', name: 'read', arguments: {path: 'a'}}],
        'toolUse',
      ),
    },
    {
      type: 'message',
      id: '3',
      parentId: '2',
      timestamp: '2026-01-01T00:00:02.000Z',
      message: {
        role: 'toolResult',
        toolCallId: 'call',
        toolName: 'read',
        content: [{type: 'text', text: 'file a'}],
        isError: false,
        timestamp: 2,
      },
    },
    {
      type: 'message',
      id: '4',
      parentId: '3',
      timestamp: '2026-01-01T00:00:03.000Z',
      message: {
        ...assistantMessage(
          [{type: 'toolCall', id: 'lost', name: 'bash', arguments: {}}],
          'aborted',
        ),
      },
    },
    {
      type: 'custom_message',
      id: '5',
      parentId: '4',
      timestamp: '2026-01-01T00:00:04.000Z',
      customType: 'hidden',
      content: 'not shown',
      display: false,
    },
    {
      type: 'custom_message',
      id: '6',
      parentId: '5',
      timestamp: '2026-01-01T00:00:05.000Z',
      customType: 'dots.worker.event',
      content: 'shown',
      display: true,
    },
  ] as unknown as SessionEntry[];
  const items = projectTranscript(entries);
  assert.deepEqual(
    items.map(item => item.kind),
    ['skill', 'user', 'assistant', 'tool', 'assistant', 'tool', 'custom'],
  );
  const [read, lost] = items.filter(item => item.kind === 'tool');
  assert.equal(read?.kind === 'tool' && read.result?.isError, false);
  assert.equal(lost?.kind === 'tool' && lost.result?.isError, true);
  assert.match(
    JSON.stringify(lost?.kind === 'tool' && lost.result?.content),
    /Operation aborted/,
  );
});

test('transcript renders saved records with Pi components and shows a live response until it is saved', t => {
  const file = path.join(scratch(t), 'session.jsonl');
  const session = new SessionFixture(file, '/fixture');
  session.user('SAVED_PROMPT');
  session.assistant(
    [
      {type: 'text', text: 'SAVED_REPLY'},
      {
        type: 'toolCall',
        id: 'call',
        // An unstyled tool's shared shell shows running output collapsed.
        name: 'fixture_tool',
        arguments: {command: 'echo SAVED_COMMAND'},
      },
    ],
    'toolUse',
  );
  let renders = 0;
  const tui = {
    requestRender: () => {
      renders++;
    },
  } as unknown as TUI;
  const transcript = new WorkerTranscript(
    tui,
    new SessionFollower(file),
    options,
  );
  const shown = () => plain(transcript.render(100));
  assert.match(shown(), /SAVED_PROMPT/);
  assert.match(shown(), /SAVED_REPLY/);
  assert.match(shown(), /SAVED_COMMAND/);

  transcript.record({
    type: 'tool_execution_start',
    toolCallId: 'call',
    toolName: 'fixture_tool',
    args: {},
  });
  transcript.record({
    type: 'tool_execution_update',
    toolCallId: 'call',
    partialResult: {content: [{type: 'text', text: 'PARTIAL_OUTPUT'}]},
  });
  assert.match(shown(), /PARTIAL_OUTPUT/);
  session.toolResult('call', 'fixture_tool', 'FINAL_OUTPUT');
  assert.equal(transcript.refresh(), true);
  assert.match(shown(), /FINAL_OUTPUT/);
  assert.doesNotMatch(shown(), /PARTIAL_OUTPUT/);

  const live = assistantMessage([]);
  live.timestamp = 42;
  transcript.record({type: 'message_start', message: live});
  transcript.record({
    type: 'message_update',
    assistantMessageEvent: {type: 'text_start', contentIndex: 0},
  });
  transcript.record({
    type: 'message_update',
    assistantMessageEvent: {
      type: 'text_delta',
      contentIndex: 0,
      delta: 'LIVE_TEXT',
    },
  });
  assert.ok(renders > 0);
  assert.match(shown(), /LIVE_TEXT/);
  session.message({...live, content: [{type: 'text', text: 'LIVE_TEXT'}]});
  transcript.refresh();
  assert.equal(
    shown().match(/LIVE_TEXT/g)?.length,
    1,
    'the saved response replaces the live one',
  );
  transcript.dispose();
});

test('transcript bounds its rows and says where the full history is', t => {
  const file = path.join(scratch(t), 'session.jsonl');
  const session = new SessionFixture(file, '/fixture');
  for (let i = 0; i < 230; i++) session.user(`row ${i}`);
  const transcript = new WorkerTranscript(
    {requestRender() {}} as unknown as TUI,
    new SessionFollower(file),
    options,
  );
  const shown = plain(transcript.render(100));
  assert.match(
    shown,
    /30 earlier rows not shown; full history:\s+\S*session\.jsonl/,
  );
  assert.doesNotMatch(shown, /row 29\b/);
  assert.match(shown, /row 229/);
  transcript.dispose();
  const windowed = new WorkerTranscript(
    {requestRender() {}} as unknown as TUI,
    new SessionFollower(file, 4096),
    options,
  );
  const tail = plain(windowed.render(100));
  assert.match(
    tail,
    /Earlier rows not shown; full history:\s+\S*session\.jsonl/,
  );
  assert.match(tail, /row 229/);
  windowed.dispose();
});

test('follower and projection extend an appended branch and rebuild after a compaction or branch change', t => {
  const file = path.join(scratch(t), 'session.jsonl');
  let clock = 0;
  const record = (
    id: string,
    parentId: string | null,
    fields: Record<string, unknown>,
  ) => {
    const timestamp = new Date(Date.UTC(2026, 0, 1, 0, 0, ++clock));
    appendFileSync(
      file,
      JSON.stringify({
        type: 'message',
        id,
        parentId,
        timestamp: timestamp.toISOString(),
        ...fields,
      }) + '\n',
    );
  };
  const user = (id: string, parentId: string | null, text: string) =>
    record(id, parentId, {
      message: {role: 'user', content: text, timestamp: clock},
    });
  const call = (id: string, parentId: string, callId: string) =>
    record(id, parentId, {
      message: {
        ...assistantMessage(
          [
            {type: 'text', text: `calling ${callId}`},
            {type: 'toolCall', id: callId, name: 'fixture_tool', arguments: {}},
          ],
          'toolUse',
        ),
        timestamp: clock,
      },
    });
  const result = (id: string, parentId: string, callId: string) =>
    record(id, parentId, {
      message: {
        role: 'toolResult',
        toolCallId: callId,
        toolName: 'fixture_tool',
        content: [{type: 'text', text: `result of ${callId}`}],
        isError: false,
        timestamp: clock,
      },
    });
  writeFileSync(
    file,
    JSON.stringify({
      type: 'session',
      id: 's',
      timestamp: '2026-01-01T00:00:00.000Z',
      cwd: '/fixture',
    }) + '\n',
  );

  const follower = new SessionFollower(file);
  let projection = new TranscriptProjection();
  let projected = 0;
  let generation = follower.session.generation;
  /** Polls and extends the projection as the transcript does; both must match a fresh read. */
  const step = (rebuilt: boolean) => {
    assert.equal(follower.poll(), true);
    const {entries} = follower.session;
    assert.equal(follower.session.generation !== generation, rebuilt);
    if (rebuilt) {
      generation = follower.session.generation;
      projection = new TranscriptProjection();
      projected = 0;
    }
    projection.append(entries.slice(projected));
    projected = entries.length;
    const full = new SessionFollower(file);
    full.poll();
    assert.deepEqual(entries, full.session.entries);
    assert.deepEqual(projection.items, projectTranscript(entries));
    return entries;
  };

  user('a', null, 'first');
  call('b', 'a', 'call-1');
  step(true);
  result('c', 'b', 'call-1');
  call('d', 'c', 'call-2');
  step(false);
  result('e', 'd', 'call-2');
  step(false);
  user('f', 'e', 'second');
  let entries = step(false);
  assert.deepEqual(
    projection.items.map(item =>
      item.kind === 'tool'
        ? `${item.call.id}:${item.result ? 'done' : 'open'}`
        : item.kind,
    ),
    ['user', 'assistant', 'call-1:done', 'assistant', 'call-2:done', 'user'],
  );

  // A record continuing an earlier entry starts a new branch.
  user('g', 'c', 'retried');
  entries = step(true);
  assert.deepEqual(texts(follower).slice(-1), ['retried']);
  assert.ok(!entries.some(entry => entry.id === 'f'));

  // A compaction rewrites the start of the branch.
  record('h', 'g', {
    type: 'compaction',
    summary: 'Earlier work summarized.',
    firstKeptEntryId: 'c',
    tokensBefore: 100,
  });
  entries = step(true);
  assert.deepEqual(
    entries.map(entry => entry.id),
    ['h', 'c', 'g'],
  );

  // Appends after a rebuild extend the rebuilt branch again.
  const transcript = new WorkerTranscript(
    {requestRender() {}} as unknown as TUI,
    new SessionFollower(file),
    options,
  );
  user('i', 'h', 'after compaction');
  transcript.refresh();
  step(false);
  const shown = plain(transcript.render(100));
  assert.match(shown, /Compacted from 100 tokens/);
  assert.match(shown, /after compaction/);
  assert.doesNotMatch(shown, /second/);
  user('j', 'g', 'other branch');
  transcript.refresh();
  step(true);
  const branched = plain(transcript.render(100));
  assert.match(branched, /other branch/);
  assert.doesNotMatch(branched, /after compaction/);
  transcript.dispose();
});
