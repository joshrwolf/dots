export type CodexJobStatus = 'running' | 'done' | 'failed' | 'stopped'

export type CodexJob = {
  id: string
  title: string
  model?: string
  sandbox: string
  isBackground: boolean
  status: CodexJobStatus
  startedAt: number
  endedAt?: number
  sessionId?: string
  lines: string[]
}

declare module 'claude-code' {
  interface PluginState {
    'codex-agent': { jobs: CodexJob[] }
  }
}
