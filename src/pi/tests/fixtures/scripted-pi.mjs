// Hand-authored scripted `pi` for workers-lifecycle.test.mjs: speaks the RPC
// protocol over stdio without a model or credentials, and imports only Node
// built-ins so each child starts quickly.
import {writeFileSync} from 'node:fs';
import {tmpdir} from 'node:os';
import path from 'node:path';

const args = process.argv.slice(2);
if (
  args.includes('--model') &&
  args[args.indexOf('--model') + 1] === 'broken/model'
) {
  process.stderr.write('Model not found\n');
  process.exit(2);
}
writeFileSync(
  path.join(
    tmpdir(),
    'dots-fake-pi-args-' + (process.env.DOTS_WORKER_ID || 'root'),
  ),
  JSON.stringify(args),
);
writeFileSync(
  path.join(
    tmpdir(),
    'dots-fake-pi-env-' + (process.env.DOTS_WORKER_ID || 'root'),
  ),
  JSON.stringify({
    rex: process.env.REX_SESSION ?? null,
    block: process.env.REX_BLOCK ?? null,
    dots: process.env.DOTS_REX_SESSION ?? null,
  }),
);
const sessionFile = args.includes('--session')
  ? args[args.indexOf('--session') + 1]
  : args[args.indexOf('--session-dir') + 1] + '/session.jsonl';
const out = o => process.stdout.write(JSON.stringify(o) + '\n');
let active = false;
let pendingSettle;
const settle = (text, final = true) => {
  active = false;
  global.last = text;
  if (final)
    out({
      type: 'message_end',
      message: {
        role: 'assistant',
        content: [{type: 'text', text}],
        stopReason: 'stop',
        ...(text.startsWith('OUTCOME first line')
          ? {usage: {totalTokens: 60_000, cost: {total: 0.125}}}
          : {}),
      },
    });
  out({type: 'agent_settled', aborted: false});
};
let buffer = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', chunk => {
  buffer += chunk;
  let i;
  while ((i = buffer.indexOf('\n')) !== -1) {
    const cmd = JSON.parse(buffer.slice(0, i));
    buffer = buffer.slice(i + 1);
    const ok = data =>
      out({
        id: cmd.id,
        type: 'response',
        command: cmd.type,
        success: true,
        data,
      });
    if (cmd.type === 'prompt') {
      if (cmd.message === 'reject') {
        out({
          id: cmd.id,
          type: 'response',
          success: false,
          error: 'rejected continuation',
        });
        continue;
      }
      if (cmd.message === 'handled') {
        ok({disposition: 'handled'});
        continue;
      }
      if (cmd.message === 'no-ack') {
        active = true;
        continue;
      }
      if (active && cmd.streamingBehavior === 'steer') {
        ok({disposition: 'queued'});
        clearTimeout(pendingSettle);
        pendingSettle = setTimeout(() => settle('steered: ' + cmd.message), 50);
        continue;
      }
      active = true;
      out({type: 'agent_start'});
      if (cmd.message === 'delayed-ack') {
        setTimeout(() => {
          ok({disposition: 'started'});
          settle('delayed done');
        }, 150);
        continue;
      }
      ok({disposition: 'started'});
      if (cmd.message === 'provider-error') {
        out({
          type: 'message_end',
          message: {
            role: 'assistant',
            content: [],
            stopReason: 'error',
            errorMessage: 'provider unavailable',
          },
        });
        settle('', false);
        continue;
      }
      if (cmd.message === 'aborted:partial') {
        out({
          type: 'message_end',
          message: {
            role: 'assistant',
            content: [{type: 'text', text: 'partial answer'}],
            stopReason: 'aborted',
          },
        });
        active = false;
        out({type: 'agent_settled', aborted: true});
        continue;
      }
      if (cmd.message === 'missing:final') {
        active = false;
        out({type: 'agent_settled', aborted: false});
        continue;
      }
      out({
        type: 'tool_execution_start',
        toolCallId: 't',
        toolName: 'bash',
        args: {},
      });
      out({
        type: 'message_update',
        usage: {totalTokens: 7},
        assistantMessageEvent: {type: 'text_delta', delta: 'x'},
      });
      const m = cmd.message;
      global.ignoreAbort = m === 'hold:ignore-abort';
      if (m === 'delay-result') {
        global.delayRead = true;
        setTimeout(() => settle('old result'), 30);
        continue;
      }
      if (m === 'two-questions') {
        out({
          type: 'extension_ui_request',
          id: 'q1',
          method: 'input',
          title: 'first question',
        });
        out({
          type: 'extension_ui_request',
          id: 'q2',
          method: 'input',
          title: 'second question',
        });
        continue;
      }
      if (m === 'crash-question') {
        out({
          type: 'extension_ui_request',
          id: 'dead-question',
          method: 'input',
          title: 'question before crash',
          timeout: 600000,
        });
        // Crash once the owner knows the session, so the worker stays resumable.
        global.crashAfterState = true;
        continue;
      }
      if (m === 'expired-question') {
        out({
          type: 'extension_ui_request',
          id: 'expired',
          method: 'input',
          title: 'expires',
          timeout: 30,
        });
        setTimeout(
          () => out({type: 'tool_execution_end', toolName: 'message'}),
          40,
        );
        continue;
      }
      if (m.startsWith('ask:')) {
        out({
          type: 'extension_ui_request',
          id: 'ui-9',
          method: 'input',
          title: m.slice(4),
        });
        return;
      }
      if (m.startsWith('confirm:')) {
        out({
          type: 'extension_ui_request',
          id: 'ui-c',
          method: 'confirm',
          title: m.slice(8),
        });
        return;
      }
      if (m.startsWith('retry:')) {
        out({
          type: 'auto_retry_start',
          attempt: 1,
          maxAttempts: 3,
          delayMs: 100,
          errorMessage: '529 overloaded\nbody',
        });
        setTimeout(() => {
          out({type: 'auto_retry_end', success: true, attempt: 2});
          settle('recovered');
        }, 300);
        return;
      }
      // Runs until steered, aborted, or stopped: never settles on its own.
      if (m.startsWith('hold:')) return;
      if (m.startsWith('slow:')) {
        pendingSettle = setTimeout(() => settle('slow done'), 400);
        return;
      }
      if (m.startsWith('long:')) {
        settle(
          'OUTCOME first line\n' +
            Array.from({length: 200}, (_, i) => 'detail line ' + i).join('\n'),
        );
        return;
      }
      if (m.startsWith('spontaneous:')) {
        settle('first answer');
        setTimeout(() => {
          active = true;
          out({type: 'agent_start'});
          setTimeout(() => settle('later chatter'), 80);
        }, 120);
        return;
      }
      if (m.startsWith('headline:')) {
        const thinking = heading =>
          out({
            type: 'message_update',
            assistantMessageEvent: {
              type: 'thinking_end',
              content: `**${heading}**`,
            },
          });
        const progress = text =>
          out({
            type: 'extension_ui_request',
            id: 'headline-note',
            method: 'notify',
            message: 'dots.message:' + JSON.stringify({to: 'parent', text}),
          });
        thinking('Inspecting API');
        if (m === 'headline:long-note') progress('x'.repeat(5000));
        if (m === 'headline:progress') {
          progress('Implementation complete');
          thinking('Checking tests');
          progress('Validation complete');
          thinking('Late reasoning');
        }
        out({
          type: 'message_end',
          message: {
            role: 'assistant',
            stopReason: 'stop',
            content: [
              ...(m === 'headline:stream'
                ? []
                : [{type: 'thinking', thinking: '**Final reasoning**'}]),
              {type: 'text', text: 'Headline fixture complete'},
            ],
          },
        });
        settle('Headline fixture complete', false);
        return;
      }
      if (m.startsWith('tell-parent:')) {
        out({
          type: 'extension_ui_request',
          id: 'n1',
          method: 'notify',
          message:
            'dots.message:' + JSON.stringify({to: 'parent', text: m.slice(12)}),
        });
        settle('told');
        return;
      }
      if (m.startsWith('tell-sibling:')) {
        const [to, ...rest] = m.slice(13).split(' ');
        out({
          type: 'extension_ui_request',
          id: 'n2',
          method: 'notify',
          message: 'dots.message:' + JSON.stringify({to, text: rest.join(' ')}),
        });
        settle('relayed');
        return;
      }
      settle('done: ' + m);
    } else if (cmd.type === 'extension_ui_response') {
      if (cmd.id === 'q1') {
        out({type: 'tool_execution_end', toolName: 'message'});
        continue;
      }
      if (cmd.id === 'q2') {
        settle('both answered');
        continue;
      }
      if (cmd.id === 'expired') continue;
      settle(
        cmd.id === 'ui-c'
          ? 'confirmed=' + cmd.confirmed
          : 'answered with ' + cmd.value,
      );
    } else if (cmd.type === 'steer') {
      ok({disposition: 'queued'});
      setTimeout(() => settle('steered: ' + cmd.message), 50);
    } else if (cmd.type === 'abort') {
      if (global.ignoreAbort) return;
      ok({});
      out({type: 'agent_settled', aborted: true});
    } else if (cmd.type === 'get_state') {
      if (global.delayRead && global.last === 'old result') {
        global.delayRead = false;
        writeFileSync(sessionFile + '.reading', 'ready');
        setTimeout(() => ok({sessionFile, isStreaming: active}), 150);
        continue;
      }
      if (global.crashAfterState) {
        process.stdout.write(
          JSON.stringify({
            id: cmd.id,
            type: 'response',
            command: cmd.type,
            success: true,
            data: {sessionFile, isStreaming: active},
          }) + '\n',
          () => process.exit(1),
        );
        continue;
      }
      ok({sessionFile, isStreaming: active});
    } else {
      ok({});
    }
  }
});
process.stdin.on('end', () => process.exit(0));
