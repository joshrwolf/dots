import { CustomMessageComponent, ToolExecutionComponent } from "@earendil-works/pi-coding-agent";
import type { Component } from "@earendil-works/pi-tui";
import { rendererResolver } from "../extensions/ui/index.ts";
import { renderJobMessage } from "../extensions/jobs/transcript.ts";
import { emptyFacts, summary, type Job } from "../extensions/jobs/protocol.ts";
import { renderEvent } from "../extensions/workers/present.ts";
import { renderWake } from "../extensions/wake/present.ts";
import type { ScenarioContext } from "./scenarios.ts";

/** Native host components through the shipping resolver, not hand-picked cards.
 * Fixtures supply records only: no tool execution or extension runtime starts. */
export function transcript(context: ScenarioContext, phase: number, expanded: boolean, job: Job): Component[] {
  const resolve = rendererResolver(context.syntax);
  function tool(name: string, args: unknown, text: string, details?: unknown, isError = false): ToolExecutionComponent {
    const view = new ToolExecutionComponent(name, `fixture-${name}`, args, { outputPad: 0 }, resolve(name, () => undefined), context.tui, "/workspace/example");
    view.setArgsComplete(); view.markExecutionStarted();
    view.updateResult({ content: [{ type: "text", text }], details, isError });
    view.setExpanded(expanded);
    return view;
  }
  const accepted = { ...job, state: "accepted" as const, finishedAt: null, facts: emptyFacts(), supervisorExit: null, observationLoss: null };
  const call = tool("job_start", { command: job.command }, summary(accepted), accepted);
  const completion = new CustomMessageComponent({ role: "custom", customType: "dots-job-result", timestamp: 0, display: true,
    content: summary(job), details: phase === 3 ? { id: job.id, state: job.state } : { id: job.id, state: job.state, job },
  }, renderJobMessage, undefined, 0);
  completion.setExpanded(expanded);
  const worker = new CustomMessageComponent({ role: "custom", customType: "dots.worker.event", timestamp: 0, display: true,
    content: "Worker review completed\n\nReviewed the renderer. No files changed.\nDetails remain available when expanded.",
    details: { id: "w-example", label: "review", kind: "completed", elapsed: "20s" },
  }, (message, options, theme) => renderEvent({ ...message, details: { id: "w-example", label: "review", kind: "completed", elapsed: "20s" } }, options, theme), undefined, 0);
  worker.setExpanded(expanded);
  const wake = new CustomMessageComponent({ role: "custom", customType: "dots.wake", timestamp: 0, display: true,
    content: "Follow-up\n\nReview the renderer result.\nCheck output: example check finished.",
    details: { id: "wake-example", fired: 1, limit: 1, late: 0, text: "Review the renderer result", exit: phase === 2 ? 1 : 0 },
  }, (message, options, theme) => renderWake({ ...message, details: { id: "wake-example", fired: 1, limit: 1, late: 0, text: "Review the renderer result", exit: phase === 2 ? 1 : 0 } }, options, theme), undefined, 0);
  wake.setExpanded(expanded);
  return [
    tool("read", { path: "src/report.ts", offset: 1, limit: 2 }, "const title = 'Example';\nexport { title };\n\n[8 more lines in file. Use offset=3 to continue.]"),
    tool("bash", { command: "npm test" }, phase === 2 ? "Renderer failed\n\nCommand exited with code 1" : "2 checks passed", undefined, phase === 2),
    call, ...(phase === 1 ? [] : [completion]),
    tool("worker_spawn", { label: "review", task: "Review the fixture" }, "Worker review spawned", { label: "review", id: "w-example", state: "running", processState: "alive", quiet: false }),
    ...(phase === 1 ? [] : [worker]),
    tool("wake", { text: "Review the renderer result", in: "15m" }, "Follow-up scheduled", { text: "Review the renderer result", active: true, in: "15m" }),
    ...(phase === 1 ? [] : [wake]),
    tool("example_lookup", { query: "renderer" }, "A third-party tool without custom renderers.\nIt uses the same transcript shell.\nNo request was sent."),
  ];
}
