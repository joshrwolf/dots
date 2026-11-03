// Rex observation and pane wakes against a real, disposable Rex server. The
// fixture isolates every Rex process and refuses any address outside its own
// directory; see tests/fixtures/rex-server.ts.
import assert from 'node:assert/strict';
import {setTimeout as sleep} from 'node:timers/promises';
import test from 'node:test';
import {systemClock} from '../../src/host/clock.ts';
import {
  RexError,
  type PaneSnapshot,
  type RexEvent,
} from '../../src/host/rex.ts';
import type {StreamState} from '../../src/host/stream.ts';
import {RexServer, b64, rexDirectory, until} from '../fixtures/rex-server.ts';
import {harness} from '../wake-fixture.mjs';

const binDir = rexDirectory();
const skip = binDir === undefined ? 'rex is not on PATH' : false;

async function withServer(
  fn: (server: RexServer) => Promise<void>,
  {restore = false} = {},
): Promise<void> {
  assert.ok(binDir);
  const server = new RexServer(binDir);
  try {
    await server.start({restore});
    await fn(server);
  } finally {
    await server.dispose();
  }
}

/** The wake extension in a session harness, observing through `server` only. */
async function wakes(server: RexServer) {
  const h = harness(systemClock, undefined, {rex: server.observer(), env: {}});
  await h.handlers.session_start({reason: 'startup'}, h.ctx);
  return h;
}

/** Waits for the event stream to connect; anything it missed is reconciled from a snapshot. */
async function streaming(server: RexServer): Promise<void> {
  await until('the event stream', () => server.launched.length > 0);
  await sleep(600);
}

const present = (snapshot: PaneSnapshot) => {
  assert.ok(snapshot.present, 'the block is present');
  return snapshot;
};

test(
  'resolvePane and snapshotPane read a real terminal block',
  {skip, timeout: 60_000},
  () =>
    withServer(async server => {
      const session = await server.session('work');
      const block = await server.run(
        session,
        'build',
        server.program(
          `report 'state=working:msg=${b64('compiling')}'`,
          'hold finish',
          `report 'state=done:msg=${b64('built')}'`,
          'exit 3',
        ),
      );
      const rex = server.observer();
      try {
        const binding = await rex.resolvePane({
          target: 'build',
          session: 'work',
          host: server.address,
        });
        assert.deepEqual(
          {...binding, where: binding.where.replace(/\d+/, 'N')},
          {
            host: server.address,
            session,
            sessionLabel: 'work',
            block,
            label: 'build',
            where: 'window N',
          },
        );
        const bySuffix = await rex.resolvePane({
          target: block.slice(-8),
          session,
          host: server.address,
        });
        assert.equal(bySuffix.block, block, 'an ID suffix resolves');

        const running = present(
          await until('the working record', async () => {
            const snapshot = await rex.snapshotPane(binding);
            return snapshot.present && snapshot.record?.state === 'working'
              ? snapshot
              : undefined;
          }),
        );
        assert.equal(running.instance, await server.instance());
        assert.equal(running.exit, null);
        assert.equal(running.record?.msg, 'compiling');
        assert.equal(typeof running.record?.updatedAt, 'string');
        assert.equal(typeof running.record?.owner?.pid, 'number');
        assert.equal(typeof running.child?.pid, 'number');

        server.step('finish');
        const exited = present(
          await until('the exit', async () => {
            const snapshot = await rex.snapshotPane(binding);
            return snapshot.present && snapshot.exit ? snapshot : undefined;
          }),
        );
        assert.deepEqual(exited.exit, {code: 3, error: 'exit status 3'});
        assert.equal(exited.record?.state, 'done', 'done survives the exit');
        assert.equal(exited.record?.msg, 'built');
        assert.equal(exited.child, undefined, 'no program runs after exit');

        await server.onboarding(session, 'intro');
        await assert.rejects(
          rex.resolvePane({target: 'intro', session, host: server.address}),
          /block intro \([0-9a-f]{8}\) is a com\.superlogical\.onboarding block, not a terminal/,
        );
        await assert.rejects(
          rex.resolvePane({target: 'nope', session, host: server.address}),
          /no block "nope" in session work; blocks: .*build \(/,
        );
        await assert.rejects(
          rex.resolvePane({
            target: 'build',
            session: 'nowhere',
            host: server.address,
          }),
          /no Rex session "nowhere"; sessions: work \(/,
        );

        await server.api('block.close', {session_id: session, block_id: block});
        assert.deepEqual(await rex.snapshotPane(binding), {
          present: false,
          instance: running.instance,
          reason: 'block closed',
        });
        await server.api('session.destroy', {session_id: session});
        assert.deepEqual(await rex.snapshotPane(binding), {
          present: false,
          instance: running.instance,
          reason: 'session destroyed',
        });

        const absent = `unix://${server.dir}/absent.sock`;
        server.owned(absent);
        await assert.rejects(
          rex.snapshotPane({...binding, host: absent}),
          (error: unknown) =>
            error instanceof RexError && error.code === undefined,
          'an unreachable server is not an API error',
        );
      } finally {
        await rex.close();
      }
    }),
);

test(
  'the event stream parses program status, removal and exit into observer shapes',
  {skip, timeout: 60_000},
  () =>
    withServer(async server => {
      const rex = server.observer();
      const events: RexEvent[] = [];
      const states: StreamState[] = [];
      let opens = 0;
      let syncs = 0;
      const stop = rex.watch(server.address, {
        open: () => opens++,
        sync: () => syncs++,
        event: event => events.push(event),
        state: state => states.push(state),
      });
      try {
        await until('the first sync', () => syncs > 0);
        const session = await server.session('work');
        const block = await server.run(
          session,
          'build',
          server.program(
            "report 'state=working'",
            "report 'state=working:id=job/x'",
            'hold finish',
            `report 'state=done:msg=${b64('built')}'`,
            'exit 2',
          ),
        );
        const onBlock = () =>
          events.filter(event => 'block' in event && event.block === block);
        await until('the working records', () => onBlock().length >= 2);
        server.step('finish');
        await until('the exit', () =>
          onBlock().some(event => event.type === 'child_exited'),
        );
        const shapes = onBlock().map(event => {
          if (event.type !== 'program_status_changed') return event;
          const {updatedAt, owner, ...record} = event.record;
          assert.equal(typeof updatedAt, 'string');
          // A report read after its process exited has no owner.
          if (record.state === 'working')
            assert.equal(typeof owner?.pid, 'number');
          return {...event, record};
        });
        assert.deepEqual(shapes, [
          {
            type: 'program_status_changed',
            session,
            block,
            id: '',
            record: {state: 'working'},
          },
          {
            type: 'program_status_changed',
            session,
            block,
            id: 'job/x',
            record: {state: 'working'},
          },
          {
            type: 'program_status_changed',
            session,
            block,
            id: '',
            record: {state: 'done', msg: 'built'},
          },
          // Exit drops working records and keeps done ones; the removal comes first.
          {
            type: 'program_status_removed',
            session,
            block,
            ids: ['job/x'],
            reason: 'child_exited',
          },
          {
            type: 'child_exited',
            session,
            block,
            exit: {code: 2, error: 'exit status 2'},
          },
        ]);

        await server.api('block.close', {session_id: session, block_id: block});
        await until('the close', () =>
          onBlock().some(event => event.type === 'block_closed'),
        );
        await server.api('session.destroy', {session_id: session});
        await until('the destruction', () =>
          events.some(
            event =>
              event.type === 'session_destroyed' && event.session === session,
          ),
        );
        assert.equal(opens, 1);
        assert.equal(states[0]?.status, 'connected');
      } finally {
        stop();
        await rex.close();
      }
    }),
);

test(
  'pane wakes fire on done and exit, and end when status stops',
  {skip, timeout: 90_000},
  () =>
    withServer(async server => {
      const session = await server.session('work');
      const programs = {
        done: server.program(
          "report 'state=working'",
          'hold done',
          `report 'state=done:msg=${b64('built')}'`,
          'hold never',
        ),
        final: server.program(
          "report 'state=working'",
          'hold final',
          `report 'state=done:msg=${b64('shipped')}'`,
          'exit 0',
        ),
        exit: server.program("report 'state=working'", 'hold exit', 'exit 3'),
        quit: server.program("report 'state=working'", 'hold quit', 'exit 0'),
        clear: server.program(
          "report 'state=working'",
          'hold clear',
          "report 'state=clear'",
          'hold never',
        ),
      };
      for (const [label, script] of Object.entries(programs))
        await server.reported(
          session,
          await server.run(session, label, script),
          'working',
        );
      const h = await wakes(server);
      try {
        const set = async (target: string, until: string[]) => {
          const text: string = await h.text('wake', {
            text: `Review ${target}`,
            pane: {target, session: 'work', host: server.address},
            until,
            for: '10m',
          });
          assert.match(text, /Status now: working\./);
        };
        await set('done', ['done']);
        await set('final', ['done']);
        await set('exit', ['exit']);
        await set('quit', ['done']);
        await set('clear', ['done']);
        await streaming(server);

        const fire = async (step: string, count: number) => {
          server.step(step);
          await until(`the ${step} fire`, () => h.messages.length >= count);
          return h.messages[count - 1].message;
        };

        const done = await fire('done', 1);
        assert.equal(done.details.outcome, 'matched');
        assert.match(done.content, /matched: status done \(was working\)/);
        assert.match(done.content, /status now: done · msg "built"/);
        assert.match(done.content, /This wake has finished/);

        // Done right before exit: Rex may omit the owner; it is the same program.
        const final = await fire('final', 2);
        assert.equal(final.details.outcome, 'matched');
        assert.match(final.content, /matched: status done \(was working\)\n/);

        const exit = await fire('exit', 3);
        assert.equal(exit.details.outcome, 'matched');
        assert.equal(exit.details.exit, 3);
        assert.match(
          exit.content,
          /matched: the block's program exited with code 3 \("exit status 3"\)/,
        );

        const quit = await fire('quit', 4);
        assert.equal(quit.details.outcome, 'ended');
        assert.equal(quit.details.exit, 0);
        assert.match(
          quit.content,
          /ended: the block's program exited with code 0/,
        );

        const clear = await fire('clear', 5);
        assert.equal(clear.details.outcome, 'ended');
        assert.match(
          clear.content,
          /ended: the program stopped reporting status without reaching a waited-for state/,
        );

        await sleep(500);
        assert.equal(h.messages.length, 5, 'one message per wake');
        assert.deepEqual(await h.call('wake_list', {}), []);
      } finally {
        await h.handlers.session_shutdown({}, h.ctx);
      }
    }),
);

test(
  'a restored Rex server keeps block IDs with fresh shells, and ends the wake',
  {skip, timeout: 90_000},
  () =>
    withServer(
      async server => {
        const session = await server.session('work');
        const block = await server.run(
          session,
          'build',
          server.program("report 'state=working'", 'hold never'),
        );
        await server.reported(session, block, 'working');
        const target = {
          session_id: session,
          block_id: block,
          method: 'process',
        };
        const before = (await server.api('block.method', target)) as {
          child: {pid: number};
        };
        const h = await wakes(server);
        try {
          await h.call('wake', {
            text: 'Review the build',
            pane: {target: 'build', session: 'work', host: server.address},
            until: ['done', 'exit'],
            for: '10m',
          });
          await streaming(server);
          const instance = await server.instance();
          await server.stop();
          const restarted = await server.start({restore: true});
          assert.notEqual(restarted, instance, 'a new server run');
          const blocks = (await server.api('session.list_blocks', {
            session_id: session,
          })) as {blocks: {block_id: string}[]};
          assert.ok(
            blocks.blocks.some(listed => listed.block_id === block),
            'the block ID survives the restore',
          );
          const after = (await server.api('block.method', target)) as {
            last_exit: unknown;
            child: {pid: number} | null;
          };
          assert.equal(after.last_exit, null);
          assert.notEqual(after.child?.pid, before.child.pid, 'a fresh shell');

          await until('the end', () => h.messages.length >= 1, 20_000);
          const [{message}] = h.messages;
          assert.equal(message.details.outcome, 'ended');
          assert.match(
            message.content,
            /ended: the Rex server restarted, so the program is gone/,
          );
          assert.deepEqual(await h.call('wake_list', {}), []);
        } finally {
          await h.handlers.session_shutdown({}, h.ctx);
        }
      },
      {restore: true},
    ),
);

test(
  'a dropped event stream is shown, reconnects and reconciles without a duplicate fire',
  {skip, timeout: 90_000},
  () =>
    withServer(async server => {
      const session = await server.session('work');
      const block = await server.run(
        session,
        'build',
        server.program(
          "report 'state=working'",
          'hold first',
          `report 'state=done:msg=${b64('one')}'`,
          'hold second',
          "report 'state=working'",
          `report 'state=done:msg=${b64('two')}'`,
          'hold never',
        ),
      );
      await server.reported(session, block, 'working');
      const h = await wakes(server);
      const listed = async () =>
        (await h.call('wake_list', {}))[0] as {disconnected: string | null};
      const drop = async () => {
        const child = server.launched.at(-1);
        assert.ok(child);
        child.kill('SIGKILL');
        return until('the disconnection', async () => {
          const {disconnected} = await listed();
          return disconnected ?? undefined;
        });
      };
      try {
        await h.call('wake', {
          text: 'Review the build',
          pane: {target: 'build', session: 'work', host: server.address},
          until: ['done'],
          limit: 3,
          for: '10m',
        });
        await streaming(server);
        server.step('first');
        await until('the live fire', () => h.messages.length >= 1);
        assert.match(
          h.messages[0].message.content,
          /matched: status done \(was working\)/,
        );

        // Nothing changes while the stream is down: no second fire.
        assert.match(await drop(), /terminated by SIGKILL/);
        assert.match(await h.text('wake_list', {}), /· disconnected/);
        await until(
          'the reconnection',
          async () =>
            (await listed()).disconnected === null &&
            server.launched.length === 2,
        );
        await sleep(1_500);
        assert.equal(h.messages.length, 1, 'an unchanged record does not fire');

        // The program finishes again while the stream is down: the snapshot fires once.
        await drop();
        server.step('second');
        await until(
          'the reconciled fire',
          () => h.messages.length >= 2,
          15_000,
        );
        const {content, details} = h.messages[1].message;
        assert.equal(details.outcome, 'matched');
        assert.match(content, /matched: status done/);
        assert.match(content, /status now: done · msg "two"/);
        assert.equal(server.launched.length, 3);
        assert.equal((await listed()).disconnected, null);
        await sleep(1_500);
        assert.equal(h.messages.length, 2, 'no duplicate fire');
      } finally {
        await h.handlers.session_shutdown({}, h.ctx);
      }
    }),
);
