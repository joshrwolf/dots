import { UserMessageComponent, type KeybindingsManager, type Theme } from "@earendil-works/pi-coding-agent";
import { Text, Spacer, VStack, type Component, type TUI } from "@earendil-works/pi-tui";
import { Inspector, OutputView, type SyntaxService, type ToolObservation, type ToolPresentation } from "../src/index.ts";
import { transcript } from "./transcript.ts";
import { thinkingComponent } from "./thinking.ts";
import { goalFixtures } from "./goals.ts";
import { GoalInspector } from "../extensions/goal/present.ts";
import { FollowedWidget } from "../extensions/goal/dashboard.ts";
import { bashPresentation } from "../src/features/tools/bash.ts";
import { readPresentation } from "../src/features/tools/read.ts";
import { writePresentation } from "../src/features/tools/write.ts";
import { editPresentation } from "../src/features/tools/edit.ts";
import { codemodePresentation } from "../src/features/tools/codemode.ts";
import { FooterView, createHeader } from "../src/features/frame.ts";
import { JobInspector } from "../extensions/jobs/inspector.ts";
import { emptyFacts, type Job } from "../extensions/jobs/protocol.ts";
import { canStop, projectJob } from "../extensions/jobs/present.ts";
import { renderWake } from "../extensions/wake/present.ts";
import { createWidget, renderEvent, type WidgetRow } from "../extensions/workers/present.ts";

// Hand-authored data only. No production runtime, process, clock, file or API access.
export interface ScenarioContext {
  readonly syntax: SyntaxService;
  readonly tui: TUI;
  readonly bindings: Pick<KeybindingsManager, "matches">;
  readonly close: () => void;
  readonly redraw: () => void;
  readonly height?: () => number;
  readonly createEditor?: (theme: Theme) => Component;
}
export interface ScenarioView {
  readonly component: Component;
  update(phase: number, expanded: boolean, theme: Theme): void;
  canAct?(): boolean;
  act?(): number | undefined;
}
export interface Scenario {
  readonly id: string;
  readonly title: string;
  readonly phases: readonly string[];
  readonly create: (context: ScenarioContext, theme: Theme) => ScenarioView;
}
type Frame = { readonly label: string; readonly observation: ToolObservation };
const call = (args: unknown, argsComplete = true, executionStarted = true): ToolObservation => ({ slot: "call", cwd: "/workspace/example", args, argsComplete, executionStarted });
const result = (args: unknown, text: string, details: unknown = undefined, error = false, partial = false): ToolObservation => ({
  slot: "result", cwd: "/workspace/example", args, content: [{ type: "text", text }], details, error, partial, durationMs: partial ? undefined : 2400, showImages: false,
});
function tool<Model>(id: string, title: string, presentation: (syntax: SyntaxService) => ToolPresentation<Model>, frames: readonly Frame[]): Scenario {
  return { id, title, phases: frames.map((frame) => frame.label), create(context) {
    const spec = presentation(context.syntax);
    const view = spec.create(context.redraw);
    const inspector = new Inspector(context.bindings, context.close, context.redraw, context.height);
    return { component: inspector, update(phase, expanded, theme) {
      const frame = frames[phase]!;
      view.update(spec.project(frame.observation), theme, expanded);
      inspector.setContent({ title, detail: frame.label, body: view, hint: "Space next state · E expand/collapse · ↑/↓ scroll · Esc close" }, theme);
    } };
  } };
}
const path = "/workspace/example/src/日本語/report.ts";
const command = "npm test -- --reporter=spec";
const shell = { command };
const source = 'export function greet(name: string) {\n  return `Hello, ${name} 👩🏽‍💻`;\n}\n';
const editArgs = { path, edits: [{ oldText: "Hello", newText: "Welcome" }] };
const script = { code: 'const results = await Promise.all([tools.read({path: "report.ts"}), tools.bash({command: "npm test"})]);\ntext(results);' };
const nested = (status: string) => ({ calls: [
  { id: "demo/1", name: "read", args: '{"path":"report.ts"}', status: "ok" },
  { id: "demo/2", name: "bash", args: '{"command":"npm test"}', status, ...(status === "error" ? { error: "exit 1" } : {}) },
] });

export function jobFixture(phase = 1): Job {
  const job: Job = { version: 1, id: "j-00000000-0000-0000-0000-000000000001", command,
    cwd: "/workspace/example", directory: "/fixture/jobs/example", timeoutMs: 60000, startedAt: 1000,
    finishedAt: null, state: "running", stopIntent: null, facts: emptyFacts(), supervisorExit: null,
    observationLoss: null, artifactsEvicted: false };
  if (phase === 0) job.state = "accepted";
  if (phase === 2) { job.state = "stopping"; job.stopIntent = "stop"; job.facts.reason = "stop"; job.facts.termSent = true; }
  if (phase === 3 || phase === 4) {
    job.state = "finished"; job.finishedAt = 3400;
    job.facts.root = { code: phase === 4 ? 1 : 0, signal: null, error: null };
    job.supervisorExit = { code: 0, signal: null };
    job.facts.capture.streamsEnded = true; job.facts.capture.prefixFlushed = true; job.facts.capture.tailFlushed = true;
    if (phase === 4) { job.facts.capture.capped = true; job.facts.capture.droppedBytes = 1024; }
  }
  if (phase === 5) { job.state = "unknown"; job.observationLoss = "Supervisor observation lost; historical jobs cannot be stopped."; }
  return job;
}

const thinkingScenario: Scenario = {
  id: "thinking", title: "Thinking blocks", phases: ["summary", "structured detail", "streaming"],
  create(context) {
    const inspector = new Inspector(context.bindings, context.close, context.redraw, context.height);
    return { component: inspector, update(phase, expanded, theme) {
      inspector.setContent({ title: "Thinking blocks", detail: "native assistant component · E toggles thinking",
        body: thinkingComponent(phase, !expanded) }, theme);
    } };
  },
};

function goalScenario(): Scenario {
  const states = [[8, "nested groups"], [9, "closed history"], [10, "incomplete snapshot"],
    [0, "claimed"], [1, "held"], [2, "recorded complete"], [3, "multiple goals"],
    [4, "unavailable"], [5, "loading"], [6, "brief warning"], [7, "empty"]] as const;
  return { id: "goal", title: "Goal dashboard", phases: states.map(([, label]) => label), create(context) {
    const preview = new Inspector(context.bindings, context.close, context.redraw, context.height);
    const inspection = new GoalInspector(context.bindings, context.close, context.redraw, context.height);
    const widget = new FollowedWidget();
    let expanded = false;
    const component = Object.assign(new VStack(), { handleInput(data: string) { (expanded ? inspection : preview).handleInput(data); } });
    return { component, update(phase, nextExpanded, theme) {
      expanded = nextExpanded;
      const views = goalFixtures(states[phase]![0]);
      widget.update(views, views[0]!.focus.id, theme);
      component.clear();
      if (expanded) {
        inspection.setGoal(views[0]!, theme, context.redraw);
        component.addChild(inspection);
      } else {
        preview.setContent({ title: "Goal dashboard", detail: "persistent widget", body: widget,
          hint: "Space state · E inspect · T theme · ↑/↓ scroll · Esc close",
        }, theme);
        component.addChild(preview);
      }
    } };
  } };
}

export const scenarios: readonly Scenario[] = [
  { id: "frame", title: "Application frame", phases: ["ordinary", "context warning", "context critical", "unknown context"], create(context) {
    const inspector = new Inspector(context.bindings, context.close, context.redraw, context.height);
    const footer = new FooterView();
    return { component: inspector, update(phase, expanded, theme) {
      footer.update({ project: "example", branch: "feature/presentation", model: "example-model", thinking: "high", contextPercent: [25, 80, 95, undefined][phase], statuses: phase === 1 ? ["workers 1 · ?1"] : [] }, theme);
      const job = jobFixture(phase === 1 ? 1 : phase === 2 ? 4 : 3);
      job.command = "printf 'Checking presentation fixtures\\n'; npm test -- --reporter=spec; printf 'Check complete\\n'";
      if (phase === 2) job.supervisorExit = { code: null, signal: "SIGKILL" };
      const goals = new FollowedWidget();
      const followed = goalFixtures(phase === 2 ? 1 : phase === 3 ? 3 : 8);
      goals.update(followed, followed[0]!.focus.id, theme);
      const activity = phase === 1 ? createWidget(() => [{ label: "review", state: "asking", depth: 0, elapsed: "1m", calls: 4, tokens: 2000, detail: "Which revision should I compare?", idleMs: 0 }], theme) : undefined;
      inspector.setContent({ title: "Application frame", detail: "real components · fixture conversation", body: new VStack([
        createHeader(theme), new Spacer(1), new UserMessageComponent("Check the renderer and report any failures.", undefined, 0),
        thinkingComponent(0, false), new Spacer(1), new Text(theme.fg("text", "I’ll run the focused checks, then review the result."), 0, 0), new Spacer(1), ...transcript(context, phase, expanded, job),
        new Spacer(1), ...(activity ? [activity, new Spacer(1)] : []), goals, new Spacer(1), ...(context.createEditor ? [context.createEditor(theme)] : []), footer,
      ]) }, theme);
    } };
  } },
  tool("bash", "Tool output", bashPresentation, [
    { label: "receiving arguments", observation: call({ command: "npm te" }, false, false) },
    { label: "queued", observation: call(shell, true, false) },
    { label: "running", observation: call(shell) },
    { label: "streaming output", observation: result(shell, "Starting tests…\n✓ parser\n", undefined, false, true) },
    { label: "completed", observation: result(shell, "✓ parser\n✓ renderer\n2 tests passed") },
    { label: "failed / truncated", observation: result(shell, "✓ parser\n✗ renderer: expected visible diagnostic\nCommand exited with code 1", { truncation: { truncated: true }, fullOutputPath: "/fixture/output/test.log" }, true) },
    { label: "large output", observation: result(shell, Array.from({ length: 1000 }, (_, index) => `output line ${String(index).padStart(4, "0")} 日本語 👩🏽‍💻\n`).join("") + "FINAL DIAGNOSTIC: permission denied", undefined, true) },
  ]),
  tool("read", "Recorded source", readPresentation, [
    { label: "source", observation: result({ path, offset: 20 }, source) },
    { label: "missing file", observation: result({ path }, "ENOENT: report.ts is unavailable", undefined, true) },
    { label: "unknown / mixed content", observation: { ...result({ path }, "Text accompanying an image"), slot: "result", content: [{ type: "text", text: "Text accompanying an image" }, { type: "image", data: "", mimeType: "image/png" }], details: undefined, partial: false, error: false, durationMs: 10, showImages: true } },
  ]),
  tool("write", "Recorded write", writePresentation, [
    { label: "streaming arguments", observation: call({ path, content: source.slice(0, 25) }, false, false) },
    { label: "completed", observation: result({ path, content: source }, "Written") },
  ]),
  tool("edit", "Recorded diff", editPresentation, [
    { label: "comparison", observation: result(editArgs, "Applied", { diff: " 1 export function greet(name: string) {\n-2   return `Hello, ${name} 👩🏽‍💻`;\n+2   return `Welcome, ${name} 👩🏽‍💻`;\n 3 }" }) },
    { label: "missing recorded diff", observation: result(editArgs, "Applied") },
    { label: "conflict", observation: result(editArgs, "Old text did not match uniquely", undefined, true) },
  ]),
  tool("codemode", "Nested tool activity", codemodePresentation, [
    { label: "running", observation: result(script, "", nested("running"), false, true) },
    { label: "caught child failure", observation: result(script, "One check failed; inspect its diagnostic.", nested("error")) },
    { label: "missing recorded summaries", observation: result(script, "Recorded output without nested details") },
  ]),
  { id: "job", title: "Job inspection", phases: ["accepted", "running", "stopping", "completed", "failed / incomplete capture", "unknown history"], create(context, theme) {
    let job = jobFixture();
    let previous = -1;
    const view = new JobInspector(projectJob(job), theme, context.bindings, context.close, context.redraw, context.height);
    return { component: view, canAct: () => canStop(job), act: () => canStop(job) ? 2 : undefined, update(phase, _expanded, theme) {
      if (previous !== phase) { job = jobFixture(phase); previous = phase; }
      view.update(projectJob(job), theme);
    } };
  } },
  { id: "workers", title: "Worker activity", phases: ["active / asking", "quiet / failed call", "saved question"], create(context) {
    const inspector = new Inspector(context.bindings, context.close, context.redraw, context.height);
    return { component: inspector, update(phase, expanded, theme) {
      const rows: WidgetRow[] = [
        { label: "review", state: "running", depth: 0, elapsed: "2m", calls: 12, tokens: 15000, detail: "read src/report.ts", idleMs: phase === 1 ? 120000 : 0 },
        { label: "日本語", state: "asking", depth: 1, elapsed: "40s", calls: 3, tokens: 2000, detail: "Which comparison should I use?", idleMs: 0, ...(phase === 1 ? { failed: "read" } : {}) },
      ];
      const body = phase === 2 ? renderEvent({ role: "custom", customType: "dots.worker.event", timestamp: 0, display: true,
        content: "Worker question\n\nWhich comparison should I use?\nThe saved observation differs from the current checkout.",
        details: { id: "w-example", kind: "asking", label: "review" },
      }, { expanded, outputPad: 0 }, theme) : createWidget(() => rows, theme);
      inspector.setContent({ title: "Worker activity", body }, theme);
    } };
  } },
  { id: "wake", title: "Scheduled follow-up", phases: ["delivered", "late / failed check"], create(context) {
    const inspector = new Inspector(context.bindings, context.close, context.redraw, context.height);
    return { component: inspector, update(phase, expanded, theme) {
      const body = renderWake({ role: "custom", customType: "dots-wake", timestamp: 0, display: true,
        content: "Scheduled follow-up\n\nInspect the latest test result.\nCheck output: fixture check finished.",
        details: { id: "wake-example", fired: 1, limit: 3, late: phase ? 120000 : 0, every: "15m", check: "check-fixture", exit: phase ? 1 : 0, text: "Inspect the latest test result" },
      }, { expanded, outputPad: 0 }, theme);
      inspector.setContent({ title: "Scheduled follow-up", body }, theme);
    } };
  } },
  thinkingScenario,
  goalScenario(),
];
