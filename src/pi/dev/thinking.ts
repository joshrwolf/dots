import type { AssistantMessage } from "@earendil-works/pi-ai";
import { AssistantMessageComponent } from "@earendil-works/pi-coding-agent";
import { hiddenThinkingLabel, thinkingMarkdown } from "../src/features/thinking.ts";

/** Neutral recorded content; never sent to a model. */
export function thinkingFixture(phase = 0): AssistantMessage {
  const thinking = phase === 0 ? "**Reviewing dependency updates**"
    : phase === 1 ? "## Compare versions\n\nKeep the existing compatibility contract.\n\n- Check the supported runtime.\n- Review the changelog and migration notes.\n\n```ts\nconst label = '日本語 👩🏽‍💻';\n```\n\nDecision: run the focused checks before updating."
    : "Check the compatibility notes before changing the version.\n\n```ts\nconst version =";
  return { role: "assistant", content: [{ type: "thinking", thinking }],
    api: "anthropic-messages", provider: "example", model: "example", stopReason: "stop", timestamp: 0,
    usage: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, totalTokens: 0, cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 } },
  };
}

export function thinkingComponent(phase: number, hidden: boolean): AssistantMessageComponent {
  const view = new AssistantMessageComponent(undefined, hidden, undefined, hiddenThinkingLabel, 0, [thinkingMarkdown]);
  view.updateContent(thinkingFixture(phase), phase === 2);
  return view;
}
