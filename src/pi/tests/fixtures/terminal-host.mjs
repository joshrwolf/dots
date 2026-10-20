// Drives Pi's real InteractiveMode through its public `terminal` option in one
// TUI mode, then exits through Pi's own shutdown. Usage: <flow> <regular|fullscreen>.
// The terminal is virtual: Pi's renderer, input parser, overlays, reload, and
// shutdown are real; raw-mode tty handling is Pi's own ProcessTerminal concern.
import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const companion = fileURLToPath(new URL("../../", import.meta.url)).replace(/\/$/, "");
const fixtures = join(companion, "tests/fixtures");
const [flow, mode] = process.argv.slice(2);
if (!["package", "workbench", "goal"].includes(flow) || !["regular", "fullscreen"].includes(mode)) throw new Error("usage: terminal-host.mjs <package|workbench|goal> <regular|fullscreen>");

// Prefer the installed host, like the CLI acceptance; fall back to the development SDK.
const installation = join(homedir(), ".pi/agent/install");
const version = join(installation, "current-version");
const root = existsSync(version) ? join(installation, "releases", readFileSync(version, "utf8").trim(), "node_modules/@earendil-works/pi-coding-agent") : undefined;
const host = root ? await import(pathToFileURL(join(root, "dist/index.js")).href) : await import("@earendil-works/pi-coding-agent");
const tui = root ? await import(pathToFileURL(join(dirname(root), "pi-tui/dist/index.js")).href) : await import("@earendil-works/pi-tui");

/** Records output and feeds input through Pi's own sequence splitter. */
class VirtualTerminal {
  columns = 120; rows = 40; kittyProtocolActive = false;
  capture = ""; transcript = "";
  start(onInput, onResize) {
    this.buffer = new tui.StdinBuffer();
    this.buffer.on("data", onInput);
    this.buffer.on("paste", (content) => onInput(`\x1b[200~${content}\x1b[201~`));
    this.onResize = onResize;
  }
  stop() { this.buffer?.destroy(); }
  async drainInput() {}
  write(data) {
    this.capture += data; this.transcript += data;
    if (this.transcript.length > 8 * 1024 * 1024) throw new Error("terminal output exceeded bound");
  }
  moveBy(lines) { if (lines) this.write(`\x1b[${Math.abs(lines)}${lines > 0 ? "B" : "A"}`); }
  hideCursor() { this.write("\x1b[?25l"); }
  showCursor() { this.write("\x1b[?25h"); }
  clearLine() { this.write("\x1b[K"); }
  clearFromCursor() { this.write("\x1b[J"); }
  clearScreen() { this.write("\x1b[2J\x1b[H"); }
  setTitle() {}
  setProgress() {}
  setProgramStatus() {}
  input(data) { this.buffer.process(data); }
  resize(rows, columns) { this.rows = rows; this.columns = columns; this.onResize(); }
}

// Translate the workbench launcher's CLI contract strictly, so launcher drift fails here.
function workbenchOptions(args) {
  const options = { extensions: [], themes: [] };
  const booleans = new Set(["--no-session", "--offline", "--no-mcp", "--no-extensions", "--no-skills", "--no-prompt-templates", "--no-context-files", "--no-approve", "--no-themes", "--no-tools"]);
  for (let i = 0; i < args.length; i++) {
    const arg = args[i];
    if (booleans.has(arg)) options[arg] = true;
    else if (arg === "--extension") options.extensions.push(args[++i]);
    else if (arg === "--theme") options.themes.push(args[++i]);
    else if (arg === "--use-theme") options.useTheme = args[++i];
    else if (arg === "--tui-mode") options.tuiMode = args[++i];
    else throw new Error(`workbench launcher argument not understood by the terminal driver: ${arg}`);
  }
  assert.ok(options["--no-session"] && options["--no-extensions"] && options["--no-tools"] && options["--no-themes"], "workbench must stay isolated");
  return options;
}

let config;
if (flow === "workbench") {
  const { prepareWorkbench } = await import(join(companion, "dev/run.mjs"));
  const trial = prepareWorkbench({ environment: process.env, mode, scenario: "bash" });
  process.on("exit", trial.cleanup);
  // Adopt the launcher's isolated environment wholesale, keeping only the marker.
  for (const key of Object.keys(process.env)) if (!(key in trial.env) && key !== "DOTS_TUI_MARKER") delete process.env[key];
  Object.assign(process.env, trial.env);
  process.chdir(trial.cwd);
  const options = workbenchOptions(trial.args);
  assert.equal(options.tuiMode, mode);
  config = { cwd: trial.cwd, extensions: [...options.extensions, join(fixtures, "terminal-observer.ts")], themes: options.themes, noThemes: true, noTools: "all", useTheme: options.useTheme };
} else if (flow === "package") {
  config = { cwd: process.cwd(), extensions: [companion, join(fixtures, "terminal-observer.ts")], themes: [], noThemes: false };
} else {
  config = { cwd: process.cwd(), extensions: [join(fixtures, "goal-terminal.ts")], themes: [], noThemes: true };
}

const agentDir = host.getAgentDir();
const createRuntime = async ({ cwd, sessionManager, sessionStartEvent }) => {
  const services = await host.createAgentSessionServices({ cwd, agentDir,
    resourceLoaderOptions: { additionalExtensionPaths: config.extensions, additionalThemePaths: config.themes, noExtensions: true, noSkills: true,
      noPromptTemplates: true, noThemes: config.noThemes, noContextFiles: true } });
  const errors = services.resourceLoader.getExtensions().errors.map(({ path, error }) => ({ type: "error", message: `Failed to load extension "${path}": ${error}` }));
  const created = await host.createAgentSessionFromServices({ services, sessionManager, sessionStartEvent, noTools: config.noTools });
  return { ...created, services, diagnostics: [...services.diagnostics, ...errors] };
};
const runtime = await host.createAgentSessionRuntime(createRuntime, { cwd: config.cwd, agentDir, sessionManager: host.SessionManager.inMemory(config.cwd) });
assert.deepEqual(runtime.diagnostics.filter((d) => d.type === "error"), [], "startup diagnostics");
host.initTheme(runtime.services.settingsManager.getTheme(), true);

const terminal = new VirtualTerminal();
const marker = process.env.DOTS_TUI_MARKER ?? process.env.DOTS_GOAL_MARKER;
const events = () => (existsSync(marker) ? readFileSync(marker, "utf8") : "");
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const plain = () => tui.stripTerminalSequences(terminal.capture);
const shows = (text) => terminal.capture.includes(text) || plain().includes(text);
const forbidden = ["uncaughtException", "UnhandledPromiseRejection", "Failed to load extension", "ERR_MODULE_NOT_FOUND", "stale after session"];
async function wait(predicate, label) {
  const deadline = Date.now() + 15_000;
  while (Date.now() < deadline) {
    if (predicate()) return;
    await sleep(20);
  }
  throw new Error(`${flow}/${mode}: timeout waiting for ${label}\nevents: ${events()}\nscreen: ${plain().slice(-3000)}`);
}
const send = (data) => { terminal.capture = ""; terminal.input(data); };
// Commands keep the capture; `fresh` starts a new one. Pi finishes an extension
// command shortly after its handler reports closing, so commands follow a brief settle.
const command = async (text) => { await sleep(100); terminal.input(text + "\r"); };
const fresh = async (text) => { await sleep(100); send(text + "\r"); };
// Pi's shutdown exits the process; judge the whole transcript on the way out.
process.on("exit", (code) => {
  if (code !== 0) return;
  const leaked = forbidden.filter((text) => tui.stripTerminalSequences(terminal.transcript).includes(text));
  const unexpected = events().includes("UNEXPECTED_PROVIDER_TURN");
  if (leaked.length || unexpected) { console.error(`${flow}/${mode}: ${leaked.join(", ")}${unexpected ? " provider turn" : ""}`); process.exitCode = 1; }
});

const interactive = new host.InteractiveMode(runtime, { terminal, tuiMode: mode, initialThemeSetting: config.useTheme });
interactive.run().catch((error) => { console.error(error); process.exit(1); });

async function packageFlow() {
  await wait(() => events().includes("start"), "startup");
  await sleep(500);
  await command("/ui");
  await wait(() => shows("Presentation"), "packaged UI menu");
  send("\x1b");
  await sleep(300);
  for (const [rows, columns] of [[28, 60], [48, 160]]) { terminal.resize(rows, columns); await sleep(200); }
  await command("/accept-theme");
  await wait(() => events().includes("theme true"), "theme change");
  await command("/reload");
  await wait(() => events().split("start").length > 2, "package reload");
  await sleep(5_500); // Delayed refresh callbacks must not use the replaced context.
  await command("/accept-exit");
}

function workbenchState(scenario, phase, expanded) {
  const records = events().split("\n").filter((line) => line.startsWith("workbench {"));
  if (!records.length) return false;
  const latest = JSON.parse(records.at(-1).slice("workbench ".length));
  return latest.scenario === scenario && latest.phase === phase && (expanded === undefined || latest.expanded === expanded);
}
const count = (text) => events().split(text).length - 1;

async function workbenchFlow() {
  const state = workbenchState;
  await wait(() => events().includes("start"), "startup");
  await wait(() => shows("/ui-lab bash"), "prefilled native editor");
  send("\r");
  await wait(() => state("bash", 0) && shows("UI workbench"), "mounted workbench");
  const initial = mode === "regular" ? "light" : "dark";
  await wait(() => events().includes(`frame 120 ${initial}`), "terminal-matched theme");
  for (const next of ["N", "\x1b[110u"]) {
    send(next);
    await wait(() => state("read", 0) && shows("Recorded source"), "next scenario");
    send("P");
    await wait(() => shows("Tool output"), "previous scenario");
  }
  for (let phase = 1; phase <= 6; phase++) { send(" "); await wait(() => state("bash", phase), `bash phase ${phase}`); }
  send("e");
  await wait(() => shows("output line"), "expanded native content");
  send("\x1b[F");
  await wait(() => shows("DIAGNOSTIC"), "native scroll to tail diagnostic");
  send("\x1b[H");
  await wait(() => shows("output line 0000"), "native scroll to start");
  if (mode === "fullscreen") { send("\x1b[<65;60;20M"); await wait(() => shows("output line"), "fullscreen wheel scrolling"); }
  for (const [rows, columns] of [[24, 44], [48, 160]]) { terminal.resize(rows, columns); await wait(() => events().includes(`frame ${columns} `), `resize to ${columns}`); }
  send("T");
  const toggled = initial === "light" ? "dark" : "light";
  await wait(() => events().includes(`frame 160 ${toggled}`), "native theme toggle");
  send("\x1b");
  await wait(() => events().includes("workbench closed"), "editor focus restored");
  await wait(() => count(`frame 160 ${initial}`) >= 2, "original palette restored");
  for (const [scenario, expected] of [["frame", "Application frame"], ["wake", "Scheduled follow-up"], ["thinking", "Thinking blocks"]]) {
    const closed = count("workbench closed");
    await fresh(`/ui-lab ${scenario}`);
    await wait(() => state(scenario, 0) && shows(expected), `${scenario} preview`);
    if (scenario === "frame") {
      await wait(() => shows("accepted") && shows("exit 0"), "registered job receipt and completion");
      send("E"); await wait(() => state("frame", 0, true), "expanded mixed transcript");
      send("\x1b[6~"); await wait(() => shows("/fixture/jobs/example/output.log"), "expanded recorded job evidence");
      send("E"); await wait(() => state("frame", 0, false), "collapsed mixed transcript");
      send("\x1b[H"); send(" "); await wait(() => state("frame", 1), "running mixed transcript");
      send(" "); await wait(() => state("frame", 2) && shows("supervisor SIGKILL"), "cleanup distinct from root outcome");
      send("\x1b[F"); await wait(() => shows("Review the result") && shows("example_lookup"), "native editor and fallback tool");
    }
    if (scenario === "wake") { send(" "); await wait(() => shows("exit 1"), "failed wake check"); }
    if (scenario === "thinking") {
      await wait(() => shows("collapsed"), "native thinking disclosure");
      send("E"); await wait(() => shows("Reviewing dependency updates"), "expanded thinking summary");
      send(" "); await wait(() => shows("Decision: run the focused checks"), "structured thinking detail");
      send(" "); await wait(() => state("thinking", 2, true) && shows("version ="), "unfinished streaming Markdown");
      send("E"); await wait(() => shows("collapsed"), "thinking collapsed again");
    }
    send("\x1b");
    await wait(() => count("workbench closed") > closed, `${scenario} closed`);
  }
  await command("/ui-lab job");
  await wait(() => state("job", 0) && shows("Job inspection"), "actual job inspector");
  send("s"); await wait(() => shows("Simulate stopping this job?"), "native action confirmation");
  send("\x1b"); await wait(() => events().includes("confirmation false"), "confirmation cancelled");
  // Cancel returns focus to the inspector; a second confirmation proves it.
  send("s"); await wait(() => shows("Simulate stopping this job?"), "confirmation reopened after cancel");
  send("\r"); await wait(() => shows("stopping"), "approved fixture action");
  send("\x1b"); await wait(() => count("workbench closed") >= 4, "job inspector closed");
  await command("/reload");
  await wait(() => count("start") >= 2 && shows("/ui-lab bash"), "isolated reload");
  // Prove focus and input interception after reload without contacting a model.
  send("\x15fixture prompt must not reach a provider\r");
  await wait(() => events().includes("workbench input handled"), "provider-free input");
  await command("/ui-lab-exit");
}

async function goalFlow() {
  await wait(() => events().includes("start"), "startup");
  await sleep(500); // Let the editor mount before sending input in either host mode.
  await fresh("/goal focus t-123abc");
  await wait(() => shows("slices done") && shows("Unified fixture exports"), "focused widget");
  await fresh("/goal");
  await wait(() => shows("1 slices"), "inspector");
  send("2"); await wait(() => shows("brief"), "brief tab");
  send("\x1b[F"); await wait(() => shows("END_OF_GOAL_BRIEF"), "scroll to end");
  for (const [rows, columns] of [[24, 45], [48, 160]]) {
    terminal.capture = ""; terminal.resize(rows, columns);
    await wait(() => shows("Unified fixture exports"), "resize");
  }
  send("\r"); await sleep(300);
  await fresh("/goal follow t-999999");
  await wait(() => shows("2 followed") && shows("Second fixture goal"), "two followed goals");
  await fresh("/goal");
  await wait(() => shows("Inspect a followed goal"), "goal selection");
  send("\r"); await wait(() => shows("1 slices"), "selected inspection");
  send("\r"); await sleep(300);
  await fresh("/reload");
  await wait(() => count("start") >= 2 && shows("2 followed"), "reload followed set");
  await fresh("/goal unfollow t-999999");
  await wait(() => shows("Unfollowed t-999999") && shows("Unified fixture exports"), "remove one follow");
  await fresh("/goal unfocus"); await wait(() => shows("Goal unfocused"), "unfocus");
  await fresh("/goal show"); await wait(() => shows("No focused goal"), "unfocused inspection");
  await fresh("/goal-fixture-exit");
}

try {
  await { package: packageFlow, workbench: workbenchFlow, goal: goalFlow }[flow]();
  await sleep(15_000);
  throw new Error(`${flow}/${mode}: Pi did not shut down`);
} catch (error) {
  console.error(error);
  process.exit(1);
}
