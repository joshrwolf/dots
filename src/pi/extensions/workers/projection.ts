import {
  parseSkillBlock,
  sessionEntryToContextMessages,
  type ParsedSkillBlock,
  type SessionEntry,
} from '@earendil-works/pi-coding-agent';

type AgentMessage = ReturnType<typeof sessionEntryToContextMessages>[number];
type Message<Role extends AgentMessage['role']> = Extract<
  AgentMessage,
  {role: Role}
>;
type ToolCall = Extract<
  Message<'assistant'>['content'][number],
  {type: 'toolCall'}
>;

/**
 * One transcript row, in the order Pi's interactive mode adds rows when it
 * renders a saved session. `key` is stable for the life of the record, so
 * one presentation can be kept per row while the session grows.
 */
export type TranscriptItem =
  | {
      readonly kind: 'user';
      readonly key: string;
      readonly text: string;
      readonly entryId: string;
      readonly timestamp: string;
    }
  | {
      readonly kind: 'skill';
      readonly key: string;
      readonly block: ParsedSkillBlock;
    }
  | {
      readonly kind: 'assistant';
      readonly key: string;
      readonly message: Message<'assistant'>;
      readonly entryId: string;
      readonly timestamp: string;
    }
  | {
      readonly kind: 'tool';
      readonly key: string;
      readonly call: ToolCall;
      /** The recorded result, or the response's failure when the call never ran. */
      readonly result: ToolOutcome | undefined;
    }
  | {
      readonly kind: 'bash';
      readonly key: string;
      readonly message: Message<'bashExecution'>;
    }
  | {
      readonly kind: 'custom';
      readonly key: string;
      readonly message: Message<'custom'>;
    }
  | {
      readonly kind: 'compaction';
      readonly key: string;
      readonly message: Message<'compactionSummary'>;
    }
  | {
      readonly kind: 'branch';
      readonly key: string;
      readonly message: Message<'branchSummary'>;
    };

export interface ToolOutcome {
  readonly content: Message<'toolResult'>['content'];
  readonly details?: unknown;
  readonly isError: boolean;
}

function userText(message: Message<'user'>): string {
  return typeof message.content === 'string'
    ? message.content
    : message.content
        .filter(block => block.type === 'text')
        .map(block => block.text)
        .join('');
}

/**
 * Rows for the active entries of a saved session. Tool calls pair with their
 * results by tool call id; a result without its call, which compaction can
 * leave behind, is not shown, as in Pi.
 */
export function projectTranscript(
  entries: readonly SessionEntry[],
): TranscriptItem[] {
  const items: TranscriptItem[] = [];
  const pending = new Map<string, number>();
  for (const entry of entries) {
    const messages = sessionEntryToContextMessages(entry);
    for (const [index, message] of messages.entries()) {
      const key = `${entry.id}:${index}`;
      switch (message.role) {
        case 'user': {
          const text = userText(message);
          if (!text) break;
          const skill = parseSkillBlock(text);
          if (skill) {
            items.push({kind: 'skill', key: `${key}:skill`, block: skill});
            if (skill.userMessage)
              items.push({
                kind: 'user',
                key,
                text: skill.userMessage,
                entryId: entry.id,
                timestamp: entry.timestamp,
              });
          } else
            items.push({
              kind: 'user',
              key,
              text,
              entryId: entry.id,
              timestamp: entry.timestamp,
            });
          break;
        }
        case 'assistant': {
          items.push({
            kind: 'assistant',
            key,
            message,
            entryId: entry.id,
            timestamp: entry.timestamp,
          });
          const failed =
            message.stopReason === 'aborted' || message.stopReason === 'error';
          for (const block of message.content) {
            if (block.type !== 'toolCall') continue;
            const result: ToolOutcome | undefined = failed
              ? {
                  content: [
                    {
                      type: 'text',
                      text:
                        message.stopReason === 'aborted'
                          ? 'Operation aborted'
                          : message.errorMessage || 'Error',
                    },
                  ],
                  isError: true,
                }
              : undefined;
            if (!failed) pending.set(block.id, items.length);
            items.push({
              kind: 'tool',
              key: `${key}:${block.id}`,
              call: block,
              result,
            });
          }
          break;
        }
        case 'toolResult': {
          const at = pending.get(message.toolCallId);
          const item = at === undefined ? undefined : items[at];
          if (item?.kind !== 'tool') break;
          pending.delete(message.toolCallId);
          items[at!] = {
            ...item,
            result: {
              content: message.content,
              details: message.details,
              isError: message.isError,
            },
          };
          break;
        }
        case 'bashExecution':
          items.push({kind: 'bash', key, message});
          break;
        case 'custom':
          if (message.display) items.push({kind: 'custom', key, message});
          break;
        case 'compactionSummary':
          items.push({kind: 'compaction', key, message});
          break;
        case 'branchSummary':
          items.push({kind: 'branch', key, message});
          break;
        default:
          break;
      }
    }
  }
  return items;
}
