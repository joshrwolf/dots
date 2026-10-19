import { test, expect, mock } from 'claude-code/testing'

const TOOL = 'mcp__codex-agent__codex'

const jsonl = (...events: unknown[]) => events.map(ev => JSON.stringify(ev)).join('\n') + '\n'

const ANSWERED = jsonl(
  { type: 'thread.started', thread_id: 'thread-1' },
  { type: 'item.started', item: { type: 'command_execution', command: "/bin/zsh -lc 'ls'", exit_code: null } },
  { type: 'item.completed', item: { type: 'command_execution', command: "/bin/zsh -lc 'ls'", exit_code: 0 } },
  { type: 'item.completed', item: { type: 'agent_message', text: 'pong' } },
  { type: 'turn.completed', usage: { input_tokens: 10, output_tokens: 2 } },
)

test('runs codex exec and returns its final message', async ($, on) => {
  mock.clock(on)
  let argv: readonly string[] = []
  let stdin: string | undefined
  on('process.spawn', async function* (_$, e) {
    argv = e.argv
    stdin = e.input
    yield { stream: 'stdout', text: ANSWERED.slice(0, 30) }
    yield { stream: 'stdout', text: ANSWERED.slice(30) }
    return { value: { code: 0, signal: null } }
  })

  const ran = await $.tool.call({ tool: TOOL, prompt: 'ping', model: 'gpt-x' })
  expect(ran.deny).toBeUndefined()
  expect(String(ran.result)).toContain('pong')
  expect(String(ran.result)).toContain('session_id: thread-1')
  expect(argv).toContain('gpt-x')
  expect(argv).toContain('sandbox_mode="read-only"')
  expect(stdin).toBe('ping')
})

test('resumes a thread when given a session_id', async ($, on) => {
  mock.clock(on)
  let argv: readonly string[] = []
  on('process.spawn', async function* (_$, e) {
    argv = e.argv
    yield { stream: 'stdout', text: ANSWERED }
    return { value: { code: 0, signal: null } }
  })
  await $.tool.call({ tool: TOOL, prompt: 'more', session_id: 'thread-1' })
  expect(argv.slice(0, 3)).toEqual(['codex', 'exec', 'resume'])
  expect(argv).toContain('thread-1')
})

test('reports failure when codex gives no answer', async ($, on) => {
  mock.clock(on)
  on('process.spawn', async function* () {
    yield { stream: 'stderr', text: 'not logged in\n' }
    return { value: { code: 1, signal: null } }
  })
  const ran = await $.tool.call({ tool: TOOL, prompt: 'x' })
  expect(ran.isError).toBe(true)
  expect(String(ran.result)).toContain('not logged in')
})

test('a background run returns at once and delivers its result as a prompt', async ($, on) => {
  mock.clock(on)
  let release = () => {}
  const gate = new Promise<void>(resolve => (release = resolve))
  let delivered = (_text: string) => {}
  const message = new Promise<string>(resolve => (delivered = resolve))
  on('process.spawn', async function* () {
    await gate
    yield { stream: 'stdout', text: ANSWERED }
    return { value: { code: 0, signal: null } }
  })
  on('prompt.submit', (_$, e) => {
    delivered(e.text)
    return { drop: 'captured by the test' }
  })

  const ran = await $.tool.call({ tool: TOOL, prompt: 'long task', background: true })
  expect(String(ran.result)).toContain('Started Codex job c1')
  release()
  const text = await message
  expect(text).toContain('Codex job c1 finished')
  expect(text).toContain('pong')
})

test('the pane lists runs with their progress', async ($, on) => {
  mock.clock(on)
  on('process.spawn', async function* () {
    yield { stream: 'stdout', text: ANSWERED }
    return { value: { code: 0, signal: null } }
  })
  await $.tool.call({ tool: TOOL, prompt: 'list files' })

  const ui = await $.ui.mount({
    plugin: 'codex-agent',
    surface: 'terminal',
    component: 'Pane',
    requestId: 'codex',
    props: {} as never,
  })
  expect(await ui.find({ type: 'Text', text: /c1 done .*list files/ })).toBeDefined()
  await ui.unmount()
})

test('Stop in the pane ends a running job', async ($, on) => {
  mock.clock(on)
  let release = () => {}
  const gate = new Promise<void>(resolve => (release = resolve))
  let delivered = (_text: string) => {}
  const message = new Promise<string>(resolve => (delivered = resolve))
  on('process.spawn', async function* () {
    yield { stream: 'stdout', text: jsonl({ type: 'thread.started', thread_id: 'thread-2' }) }
    await gate
    yield { stream: 'stdout', text: ANSWERED }
    return { value: { code: 0, signal: null } }
  })
  on('prompt.submit', (_$, e) => {
    delivered(e.text)
    return { drop: 'captured by the test' }
  })

  await $.tool.call({ tool: TOOL, prompt: 'slow', background: true })
  const ui = await $.ui.mount({
    plugin: 'codex-agent',
    surface: 'terminal',
    component: 'Pane',
    requestId: 'codex',
    props: {} as never,
  })
  await ui.press({ key: 'stop-c1' })
  release()
  const text = await message
  expect(text).toContain('stopped from the Codex pane')
  expect(text).not.toContain('pong')
  await ui.unmount()
})
