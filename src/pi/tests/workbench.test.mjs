import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import { test } from "node:test";
import { prepareWorkbench } from "../dev/run.mjs";
import { scenarios } from "../dev/scenarios.ts";

test("workbench isolates home, resources and credentials without registering or installing anything", () => {
  const environment = { PATH: process.env.PATH, TERM: "xterm-256color", COLORFGBG: "0;15", HOME: "/real-home", API_KEY: "not-a-key", NODE_OPTIONS: "--require=/untrusted", PI_CODING_AGENT_DIR: "/real-agent", PI_BIN: "/example/pi" };
  const trial = prepareWorkbench({ environment, mode: "regular", scenario: "job" });
  try {
    assert.equal(trial.binary, "/example/pi");
    assert.ok(existsSync(trial.cwd));
    assert.ok(trial.env.HOME.startsWith(trial.directory));
    assert.ok(trial.env.PI_CODING_AGENT_DIR.startsWith(trial.directory));
    assert.equal(trial.env.COLORFGBG, "0;15");
    assert.equal(trial.args[trial.args.indexOf("--use-theme") + 1], "dots-light/dots-dark");
    assert.equal(trial.env.API_KEY, undefined);
    assert.equal(trial.env.NODE_OPTIONS, undefined);
    assert.equal(trial.env.DOTS_UI_WORKBENCH, "1");
    assert.equal(trial.env.DOTS_UI_SCENARIO, "job");
    assert.ok(trial.args.includes("--no-tools"));
    assert.ok(trial.args.includes("--no-session"));
    assert.ok(trial.args.includes("--no-extensions"));
    assert.ok(trial.args.includes("--no-approve"));
    assert.ok(trial.args.includes("--offline"));
    assert.equal(trial.args.filter((arg) => arg === "--extension").length, 2);
    assert.ok(trial.args.some((arg) => arg.endsWith("extensions/ui/index.ts")));
    assert.ok(trial.args.some((arg) => arg.endsWith("dev/index.ts")));
    assert.doesNotMatch(trial.args.join(" "), /extensions\/(jobs|workers)\/index| install |register/);
    assert.equal(environment.HOME, "/real-home");
  } finally { trial.cleanup(); }
  assert.equal(existsSync(trial.directory), false);
  trial.cleanup();
  assert.throws(() => prepareWorkbench({ mode: "rpc" }), /mode/);
  assert.throws(() => prepareWorkbench({ scenario: "unknown" }), /scenario/);
});

test("every registered scenario can launch directly in either terminal mode", () => {
  for (const { id } of scenarios) for (const mode of ["regular", "fullscreen"]) {
    const trial = prepareWorkbench({ mode, scenario: id });
    try {
      assert.equal(trial.env.DOTS_UI_SCENARIO, id);
      assert.equal(trial.args[trial.args.indexOf("--tui-mode") + 1], mode);
    } finally { trial.cleanup(); }
  }
});

test("development resources are opt-in, not registered", () => {
  const manifest = JSON.parse(readFileSync(new URL("../package.json", import.meta.url), "utf8"));
  assert.ok(manifest.pi.extensions.every((entry) => !entry.startsWith("dev/")));
});
