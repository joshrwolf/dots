import { spawn } from "node:child_process";
import { mkdtempSync, mkdirSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const packageRoot = fileURLToPath(new URL("../", import.meta.url));

/** A disposable Pi home and neutral cwd, not a security sandbox. Only explicitly
 * named presentation resources load; credentials and personal state are absent. */
export function prepareWorkbench({ environment = process.env, mode = "fullscreen", scenario = "frame" } = {}) {
  if (!["regular", "fullscreen"].includes(mode)) throw new Error("mode must be regular or fullscreen");
  if (!["bash", "read", "write", "edit", "codemode", "job", "workers", "wake", "frame", "thinking"].includes(scenario)) throw new Error("Unknown workbench scenario");
  const directory = mkdtempSync(join(tmpdir(), "dots-ui-"));
  const home = join(directory, "home");
  const cwd = join(directory, "example");
  mkdirSync(home, { mode: 0o700 }); mkdirSync(cwd, { mode: 0o700 });
  const env = Object.fromEntries(["PATH", "TERM", "COLORTERM", "COLORFGBG", "TERM_PROGRAM", "LANG", "LC_ALL", "TMPDIR"].flatMap((key) => environment[key] ? [[key, environment[key]]] : []));
  Object.assign(env, { HOME: home, XDG_CONFIG_HOME: join(home, ".config"), XDG_STATE_HOME: join(home, ".local/state"), XDG_CACHE_HOME: join(home, ".cache"),
    PI_CODING_AGENT_DIR: join(home, ".pi/agent"), PI_OFFLINE: "1", DOTS_UI_WORKBENCH: "1", DOTS_UI_SCENARIO: scenario });
  return { cwd, env, binary: environment.PI_BIN || "pi", directory,
    args: ["--no-session", "--offline", "--no-mcp", "--no-extensions", "--no-skills", "--no-prompt-templates", "--no-context-files", "--no-approve", "--no-themes", "--no-tools",
      "--extension", join(packageRoot, "extensions/ui/index.ts"), "--extension", join(packageRoot, "dev/index.ts"),
      "--theme", join(packageRoot, "themes/dots-light.json"), "--theme", join(packageRoot, "themes/dots-dark.json"), "--use-theme", "dots-light/dots-dark", "--tui-mode", mode],
    cleanup: () => rmSync(directory, { recursive: true, force: true }),
  };
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  if (!process.stdin.isTTY || !process.stdout.isTTY) throw new Error("Workbench needs an interactive terminal; use :verify-terminal for automated checks.");
  const [mode = "fullscreen", scenario = "frame", ...extra] = process.argv.slice(2);
  if (extra.length) throw new Error("Usage: workbench [fullscreen|regular] [scenario]");
  const trial = prepareWorkbench({ mode, scenario });
  const child = spawn(trial.binary, trial.args, { cwd: trial.cwd, env: trial.env, stdio: "inherit" });
  const stop = () => { if (child.exitCode === null && child.signalCode === null) child.kill("SIGTERM"); };
  process.on("SIGTERM", stop);
  try {
    process.exitCode = await new Promise((resolve, reject) => {
      child.once("error", reject);
      child.once("exit", (code, signal) => resolve(code ?? (signal === "SIGTERM" ? 143 : 1)));
    });
  } finally { process.off("SIGTERM", stop); trial.cleanup(); }
}
