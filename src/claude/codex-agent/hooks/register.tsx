import { atom, read, update } from 'claude-code'
import type { EngineInterface, Register } from 'claude-code'

import type { CodexJob } from '../types'

const PANE = 'codex'
const TOOL = 'mcp__codex-agent__codex'
const SANDBOXES = ['read-only', 'workspace-write'] as const
const EFFORTS = ['minimal', 'low', 'medium', 'high', 'xhigh'] as const
const MAX_JOBS = 30
const MAX_LINES = 200

const jobs = atom({ plugin: 'codex-agent', key: 'jobs' } as const, [])

type Input = {
  prompt: string
  model?: string
  sandbox?: (typeof SANDBOXES)[number]
  effort?: (typeof EFFORTS)[number]
  cwd?: string
  session_id?: string
  background?: boolean
}

type Item = {
  type: string
  text?: string
  command?: string
  exit_code?: number | null
  changes?: { path: string }[]
  server?: string
  tool?: string
  query?: string
}

type Event =
  | { type: 'thread.started'; thread_id: string }
  | { type: 'item.started' | 'item.completed'; item: Item }
  | { type: 'turn.completed'; usage?: Record<string, number> }
  | { type: 'turn.failed' | 'error'; error?: { message?: string }; message?: string }

type Outcome = {
  answer?: string
  failure?: string
  threadId?: string
  usage?: Record<string, number>
  stderr: string
  isStopped: boolean
}

const DESCRIPTION = `Run a task on an OpenAI model as a Codex subagent (the Codex CLI, billed to the user's ChatGPT subscription). It works autonomously in its own context, can read files and run commands in a sandbox, and returns its final message plus a session_id. Its progress shows live in the Codex pane (/codex).

Use it for a second opinion from a different model family, independent review, or parallelizable research. Give it a self-contained prompt: it has none of this conversation's context.

- sandbox: "read-only" (default) or "workspace-write" (may edit files under cwd).
- model / effort: omit to use the user's Codex config defaults.
- session_id: pass a previous call's session_id to continue that Codex thread with a follow-up prompt.
- background: true returns at once with a job id; the result arrives later as a message from this plugin. Use it for long tasks you do not need to wait on.`

// Stops for this module's running children; a reload kills them all.
const stops = new Map<string, () => void>()

export const register: Register = on => {
  on('session.start', async ($, e, next) => {
    const started = await next(e)
    const now = await $.clock.now()
    await update($, jobs, list =>
      list.map(job =>
        job.status === 'running'
          ? { ...job, status: 'stopped', endedAt: now, lines: [...job.lines, 'stopped: the mod reloaded'] }
          : job,
      ),
    )
    await $.command.register({ name: 'codex', description: 'Show Codex subagent runs in a pane' })
    await $.tool.register({
      name: 'codex',
      description: DESCRIPTION,
      isDeferred: false,
      inputSchema: {
        type: 'object',
        properties: {
          prompt: { type: 'string', description: 'Self-contained task for the Codex agent.' },
          model: { type: 'string', description: 'OpenAI model id; omit for the Codex default.' },
          sandbox: { type: 'string', enum: SANDBOXES },
          effort: { type: 'string', enum: EFFORTS, description: 'Reasoning effort.' },
          cwd: { type: 'string', description: 'Working root; defaults to the session cwd.' },
          session_id: { type: 'string', description: 'Continue an earlier Codex thread.' },
          background: { type: 'boolean', description: 'Return at once; deliver the result later.' },
        },
        required: ['prompt'],
        additionalProperties: false,
      },
    })
    return started
  })

  on('command.run', { command: 'codex' }, async $ => {
    await $.ui.open({ id: PANE, title: 'Codex' })
    return { text: 'Codex pane opened.' }
  })

  on('tool.call', { tool: TOOL }, async ($, e) => {
    const input = e as unknown as Input
    const job = await startJob($, input)
    void $.ui.open({ id: PANE, title: 'Codex' }).catch(() => undefined)

    if (input.background) {
      void runJob($, job, input)
        .then(outcome => $.prompt.submit({ text: `Codex job ${job.id} finished.\n\n${report(outcome)}` }))
        .catch(() => $.ui.toast(`Codex job ${job.id} finished, but its result could not be delivered; see /codex`))
      return { result: `Started Codex job ${job.id} in the background. Its result arrives as a message when it finishes.` }
    }

    const outcome = await runJob($, job, input)
    return outcome.answer === undefined ? { result: report(outcome), isError: true } : { result: report(outcome) }
  }).catch(($, e, next) => ({ deny: `${$.plugin.name}: the codex tool failed.` }))

  on('ui.render', { component: 'Pane', requestId: PANE }, async ($, e) => {
    const { Box, Button, Text } = $.ui.resolve(e)
    const list = await read($, jobs)
    const room = Math.max(4, (e.viewport?.rows ?? 24) - 2)
    const running = list.filter(job => job.status === 'running')
    const finished = list.filter(job => job.status !== 'running').slice(-5).reverse()
    const tail = Math.max(2, Math.floor((room - finished.length) / Math.max(1, running.length)) - 1)

    return (
      <Box flexDirection="column">
        {list.length === 0 && <Text dimColor>No Codex runs yet.</Text>}
        {running.map(job => (
          <Box key={job.id} flexDirection="column">
            <Box>
              <Text color="yellow">{header(job)} </Text>
              <Button key={`stop-${job.id}`} label="Stop" onPress={() => stops.get(job.id)?.()} />
            </Box>
            {job.lines.slice(-tail).map(line => (
              <Text dimColor>  {line}</Text>
            ))}
          </Box>
        ))}
        {finished.map(job => (
          <Text key={job.id} color={job.status === 'done' ? 'green' : 'red'} dimColor={job.status === 'stopped'}>
            {header(job)}
          </Text>
        ))}
      </Box>
    )
  })
}

async function startJob($: EngineInterface, input: Input): Promise<CodexJob> {
  const list = await read($, jobs)
  const job: CodexJob = {
    id: `c${(Number(list.at(-1)?.id.slice(1)) || 0) + 1}`,
    title: firstLine(input.prompt, 60),
    model: input.model,
    sandbox: input.sandbox ?? 'read-only',
    isBackground: input.background === true,
    status: 'running',
    startedAt: await $.clock.now(),
    sessionId: input.session_id,
    lines: [],
  }
  await update($, jobs, all => [...all, job].slice(-MAX_JOBS))
  return job
}

async function runJob($: EngineInterface, job: CodexJob, input: Input): Promise<Outcome> {
  const flags = ['--json', '--skip-git-repo-check', '-c', `sandbox_mode="${job.sandbox}"`]
  if (input.model) flags.push('-m', input.model)
  if (input.effort) flags.push('-c', `model_reasoning_effort="${input.effort}"`)
  const argv = input.session_id
    ? ['codex', 'exec', 'resume', ...flags, input.session_id, '-']
    : ['codex', 'exec', ...flags, ...(input.cwd ? ['-C', input.cwd] : []), '-']

  const outcome: Outcome = { threadId: input.session_id, stderr: '', isStopped: false }
  let buffered = ''
  try {
    const child = $.process.spawn({ argv, cwd: input.cwd, input: input.prompt })
    stops.set(job.id, () => {
      outcome.isStopped = true
      void child.return(undefined as never)
    })
    for await (const { stream, text } of child) {
      if (outcome.isStopped) break
      if (stream === 'stderr') {
        outcome.stderr += text
        continue
      }
      const lines = (buffered + text).split('\n')
      buffered = lines.pop() ?? ''
      const shown: string[] = []
      for (const line of lines) {
        const ev = parse(line)
        if (!ev) continue
        if (ev.type === 'thread.started') outcome.threadId = ev.thread_id
        else if (ev.type === 'turn.completed') outcome.usage = ev.usage
        else if (ev.type === 'turn.failed' || ev.type === 'error')
          outcome.failure = ev.error?.message ?? ev.message ?? 'Codex reported an error'
        else if (ev.type === 'item.completed' && ev.item.type === 'agent_message') outcome.answer = ev.item.text
        const said = describe(ev)
        if (said) shown.push(said)
      }
      if (shown.length > 0) await patch($, job.id, one => ({ lines: [...one.lines, ...shown].slice(-MAX_LINES) }))
    }
  } catch (err) {
    outcome.failure = `could not start codex: ${String(err)}`
  } finally {
    stops.delete(job.id)
  }

  if (outcome.isStopped) outcome.failure = 'stopped from the Codex pane'
  const status = outcome.isStopped ? 'stopped' : outcome.answer === undefined ? 'failed' : 'done'
  const endedAt = await $.clock.now()
  await patch($, job.id, () => ({ status, endedAt, sessionId: outcome.threadId }))
  return outcome
}

function patch($: EngineInterface, id: string, fn: (job: CodexJob) => Partial<CodexJob>) {
  return update($, jobs, list => list.map(job => (job.id === id ? { ...job, ...fn(job) } : job)))
}

function report(outcome: Outcome): string {
  if (outcome.answer === undefined || outcome.isStopped) {
    const tail = outcome.stderr.trim().split('\n').slice(-20).join('\n')
    return `Codex failed: ${outcome.failure ?? 'no final message'}${tail ? `\n\nstderr:\n${tail}` : ''}`
  }
  const meta = [`session_id: ${outcome.threadId ?? 'unknown'}`]
  if (outcome.usage) meta.push(`tokens: ${outcome.usage.input_tokens ?? 0} in / ${outcome.usage.output_tokens ?? 0} out`)
  return `${outcome.answer}\n\n[codex ${meta.join(', ')}]`
}

function describe(ev: Event): string | undefined {
  if (ev.type === 'turn.failed' || ev.type === 'error') return `error: ${ev.error?.message ?? ev.message ?? ''}`
  if (ev.type !== 'item.started' && ev.type !== 'item.completed') return undefined
  const { item } = ev
  const isDone = ev.type === 'item.completed'
  switch (item.type) {
    case 'command_execution':
      return isDone ? `  exit ${item.exit_code ?? '?'}` : `$ ${unwrap(item.command ?? '')}`
    case 'agent_message':
      return isDone ? firstLine(item.text ?? '', 200) : undefined
    case 'reasoning':
      return isDone ? `… ${firstLine(item.text ?? '', 120)}` : undefined
    case 'file_change':
      return isDone ? `edit ${(item.changes ?? []).map(change => change.path).join(', ')}` : undefined
    case 'mcp_tool_call':
      return isDone ? undefined : `tool ${item.server}.${item.tool}`
    case 'web_search':
      return isDone ? undefined : `search ${item.query ?? ''}`
    default:
      return undefined
  }
}

function header(job: CodexJob): string {
  const took = job.endedAt === undefined ? '' : ` ${Math.round((job.endedAt - job.startedAt) / 1000)}s`
  const tags = [job.model, job.sandbox, job.isBackground ? 'bg' : undefined].filter(Boolean).join(' · ')
  return `${job.id} ${job.status}${took} [${tags}] ${job.title}`
}

// Codex runs commands through a login shell; show the command itself.
function unwrap(command: string): string {
  const match = /^\S+ -lc '(.*)'$/s.exec(command)
  return firstLine(match?.[1] ?? command, 160)
}

function firstLine(text: string, max: number): string {
  const line = text.trim().split('\n')[0] ?? ''
  return line.length > max ? `${line.slice(0, max - 1)}…` : line
}

function parse(line: string): Event | undefined {
  if (!line.trim()) return undefined
  try {
    return JSON.parse(line) as Event
  } catch {
    return undefined
  }
}
