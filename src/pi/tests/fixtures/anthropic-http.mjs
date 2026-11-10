import {createServer} from 'node:http';
import {once} from 'node:events';

/** Real local HTTP, neutral SSE fixtures. Never forwards requests outside localhost. */
export async function anthropicHttp(plan = () => 'ok') {
  const requests = [];
  const active = new Set();
  const encode = (type, fields = {}) =>
    `event: ${type}\ndata: ${JSON.stringify({type, ...fields})}\n\n`;
  const errors = [];
  const respond = async (req, res) => {
    const chunks = [];
    for await (const chunk of req) chunks.push(chunk);
    const payload = JSON.parse(Buffer.concat(chunks).toString());
    const record = {
      payload,
      headers: req.headers,
      closed: false,
      mode: undefined,
      inputTokens: 1,
      contentDelayMs: 200,
    };
    requests.push(record);
    record.mode = plan(record, requests);
    const timers = [];
    active.add(record);
    const clean = () => {
      record.closed = true;
      active.delete(record);
      for (const timer of timers) clearTimeout(timer);
    };
    res.on('close', clean);
    if (record.mode === 'no-headers') return;
    if (record.mode === 'thread-rejected') {
      res.writeHead(400, {'content-type': 'application/json'});
      res.end(
        JSON.stringify({
          type: 'error',
          error: {
            type: 'invalid_request_error',
            message: 'fixture thread not found',
            details: {error_code: 'thread_not_found'},
          },
        }),
      );
      return;
    }
    if (record.mode === 'delayed-headers')
      await new Promise(resolve => timers.push(setTimeout(resolve, 150)));
    if (res.destroyed) return;
    if (record.mode === 'http-error') {
      res.writeHead(503, {'content-type': 'application/json'});
      res.end(
        '{"type":"error","error":{"type":"overloaded_error","message":"fixture overloaded"}}',
      );
      return;
    }
    res.writeHead(200, {
      'content-type': 'text/event-stream',
      'x-fixture': 'preserved',
    });
    res.flushHeaders();
    if (record.mode !== 'silent')
      timers.push(setInterval(() => res.write(encode('ping')), 10));
    if (['stall', 'silent'].includes(record.mode)) return;
    const start = () => {
      if (res.destroyed) return;
      res.write(
        encode('message_start', {
          message: {
            id: 'fixture-message',
            type: 'message',
            role: 'assistant',
            model: payload.model,
            content: [],
            usage: {input_tokens: record.inputTokens, output_tokens: 0},
          },
        }),
      );
      if (record.mode === 'malformed') {
        res.write('event: content_block_delta\ndata: definitely-not-json\n\n');
        return;
      }
      if (record.mode === 'started-stall') return;
      timers.push(
        setTimeout(
          () => {
            if (res.destroyed) return;
            const tool = ['tool', 'spawn', 'ask'].includes(record.mode);
            if (tool) {
              const name =
                record.mode === 'spawn'
                  ? 'worker_spawn'
                  : record.mode === 'ask'
                    ? 'message'
                    : 'read';
              const input =
                record.mode === 'spawn'
                  ? {
                      label: 'inner',
                      task: 'nested-fixture',
                      model: 'inherit',
                      thinking: 'inherit',
                    }
                  : record.mode === 'ask'
                    ? {to: 'parent', text: 'fixture question', wait: true}
                    : {path: 'fixture.txt'};
              res.write(
                encode('content_block_start', {
                  index: 0,
                  content_block: {
                    type: 'tool_use',
                    id: 'fixture-call',
                    name,
                    input: {},
                  },
                }),
              );
              res.write(
                encode('content_block_delta', {
                  index: 0,
                  delta: {
                    type: 'input_json_delta',
                    partial_json: JSON.stringify(input),
                  },
                }),
              );
            } else {
              res.write(
                encode('content_block_start', {
                  index: 0,
                  content_block: {type: 'text', text: ''},
                }),
              );
              res.write(
                encode('content_block_delta', {
                  index: 0,
                  delta: {type: 'text_delta', text: 'fixture ok'},
                }),
              );
            }
            res.write(encode('content_block_stop', {index: 0}));
            res.write(
              encode('message_delta', {
                delta: {
                  stop_reason: tool ? 'tool_use' : 'end_turn',
                  stop_sequence: null,
                },
                usage: {output_tokens: 2},
              }),
            );
            res.write(encode('message_stop'));
            res.end();
          },
          record.mode === 'slow-content' ? record.contentDelayMs : 5,
        ),
      );
    };
    timers.push(setTimeout(start, record.mode === 'delayed-start' ? 30 : 0));
  };
  // A fixture failure (unreadable body, throwing plan) is a visible 500 and a recorded error, never an unhandled rejection.
  const server = createServer((req, res) => {
    respond(req, res).catch(error => {
      errors.push(error);
      if (res.headersSent) res.destroy(error);
      else
        res
          .writeHead(500, {'content-type': 'text/plain'})
          .end(`fixture failed: ${error}`);
    });
  });
  server.listen(0, '127.0.0.1');
  await once(server, 'listening');
  const url = `http://127.0.0.1:${server.address().port}/v1/messages`;
  return {
    url,
    requests,
    active,
    errors,
    fetch: (_input, init) => fetch(url, init),
    async close() {
      server.closeAllConnections();
      await new Promise(resolve => server.close(resolve));
    },
  };
}
