import {Type, type Static} from 'typebox';
import type {InputEvent} from '@earendil-works/pi-coding-agent';
import {agentEnv, type Agent} from '../../../src/host/agent.ts';
import type {AttentionSource, GoalRef} from './source.ts';

/** Interactive input messages kept for quoting; never persisted. */
const RECEIPTS = 16;
const RECEIPT_CHARS = 16_384;
const QUOTE_CHARS = 4096;

/**
 * Recent messages Pi delivered to this extension as interactive input. RPC,
 * extension and worker input never count, and a user-role message alone is
 * not evidence of where it came from. Pi passes earlier input handlers'
 * transforms on with the same source, and exposes no original text, so this
 * is the text as delivered, not proof of unmodified keystrokes. Bounded and
 * in memory; a new session starts empty.
 */
export class HumanInputs {
  private readonly texts: string[] = [];

  observe(event: Pick<InputEvent, 'source' | 'text'>): void {
    if (event.source !== 'interactive') return;
    const text = event.text.slice(0, RECEIPT_CHARS);
    if (!text.trim()) return;
    this.texts.push(text);
    if (this.texts.length > RECEIPTS) this.texts.shift();
  }

  /** The quote appears exactly in a recent interactive message. */
  attests(quote: string): boolean {
    const exact = quote.trim();
    return !!exact && this.texts.some(text => text.includes(exact));
  }

  clear(): void {
    this.texts.length = 0;
  }
}

export const RecordChatParameters = Type.Object({
  kind: Type.Union(
    [Type.Literal('answer'), Type.Literal('decision'), Type.Literal('mandate')],
    {
      description:
        'answer: a person answered an open request in chat. decision or mandate: a person established a decision or the goal mandate in chat, with no request.',
    },
  ),
  quote: Type.String({
    minLength: 1,
    maxLength: QUOTE_CHARS,
    description:
      "The smallest relevant part of the person's own message, copied exactly. It must appear verbatim in a recent interactive message in this session.",
  }),
  request: Type.Optional(
    Type.String({
      pattern: '^r-[a-f0-9]{32}$',
      description: 'For answer: the exact request ID the person answered.',
    }),
  ),
  choice: Type.Optional(
    Type.String({
      pattern: '^[a-z0-9-]{1,32}$',
      description:
        "For answer: the request's choice ID, only when the person clearly chose it.",
    }),
  ),
  interpretation: Type.Optional(
    Type.String({
      minLength: 1,
      maxLength: 4096,
      description:
        'For answer: how you read the quote as an answer to this request.',
    }),
  ),
  goal: Type.Optional(
    Type.String({
      pattern: '^t-[a-f0-9]+$',
      description:
        'For decision or mandate: the goal it belongs to, one this session follows or coordinates.',
    }),
  ),
  decision: Type.Optional(
    Type.String({
      minLength: 1,
      maxLength: 8192,
      description: 'For decision or mandate: the decision, in your words.',
    }),
  ),
});
export type RecordChat = Static<typeof RecordChatParameters>;

export const RECORD_CHAT_DESCRIPTION =
  "Record a person's chat message as their answer to an open attention request, or as a goal decision or mandate, quoting their exact words. Use only when the person plainly answered or decided; ambiguous assent needs clarification first. Never quote your own or a worker's text. A UI answer needs no recording.";

/**
 * Record one chat-established answer or decision. The quote is checked
 * against recent interactive input; the interpretation is stored separately
 * from it. This is cooperative provenance, not consent or a permission check:
 * the store still enforces request state and coordinator authority.
 */
export async function recordChat(
  params: RecordChat,
  agent: Agent,
  humans: HumanInputs,
  source: AttentionSource,
  goals: readonly GoalRef[],
  projectOf: (request: string) => string | undefined,
  signal: AbortSignal,
): Promise<string> {
  const quote = params.quote.trim();
  if (!humans.attests(quote))
    throw new Error(
      'The quote does not appear verbatim in a recent interactive message in this session. Quote their exact words, or ask them to answer in the goal popup (Alt+G).',
    );
  const provenance = `pi session ${agent.session} · interactive input`;
  const actor = {env: agentEnv(agent)};
  if (params.kind === 'answer') {
    if (!params.request || !params.interpretation)
      throw new Error('kind answer needs request and interpretation.');
    if (params.goal || params.decision)
      throw new Error(
        'kind answer takes request, choice and interpretation, not goal or decision.',
      );
    const project = projectOf(params.request);
    if (!project)
      throw new Error(
        `Request ${params.request} is not outstanding on a goal this session follows or coordinates. Check dots attention list --scope goals --goal PROJECT:GOAL.`,
      );
    const saved = await source.answer(
      project,
      params.request,
      {
        source: 'chat',
        text: quote,
        ...(params.choice ? {choice: params.choice} : {}),
        provenance,
        interpretation: params.interpretation,
      },
      actor,
      signal,
    );
    return `Saved the answer to ${saved.id}; its coordinator incorporates it.`;
  }
  if (!params.goal || !params.decision)
    throw new Error(`kind ${params.kind} needs goal and decision.`);
  if (params.request || params.choice || params.interpretation)
    throw new Error(
      `kind ${params.kind} takes goal and decision, not request, choice or interpretation.`,
    );
  const goal = goals.find(item => item.id === params.goal);
  if (!goal)
    throw new Error(
      `Goal ${params.goal} is not followed or coordinated by this session. Its goals: ${goals.map(item => item.id).join(', ') || 'none'}.`,
    );
  const recorded = await source.decide(
    goal,
    {
      kind: params.kind,
      decision: params.decision,
      human: {text: quote, source: 'chat', provenance},
    },
    actor,
    signal,
  );
  return `Recorded ${params.kind} ${recorded.id} for goal ${goal.id}.`;
}
