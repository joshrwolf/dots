import assert from "node:assert/strict";
import {copyFileSync, globSync, mkdirSync, mkdtempSync, rmSync, unlinkSync, writeFileSync} from "node:fs";
import {tmpdir} from "node:os";
import {dirname, join} from "node:path";
import {spawnSync} from "node:child_process";
import {fileURLToPath} from "node:url";
import {test} from "node:test";

const source = fileURLToPath(new URL("../", import.meta.url));

test("mise selects the actual affected deliverables", async t => {
    const root = mkdtempSync(join(tmpdir(), "dots-graph-"));
    const env = {...process.env, MISE_TRUSTED_CONFIG_PATHS: root, MISE_CEILING_PATHS: dirname(root),
        GIT_AUTHOR_NAME: "Example", GIT_AUTHOR_EMAIL: "example@example.invalid",
        GIT_COMMITTER_NAME: "Example", GIT_COMMITTER_EMAIL: "example@example.invalid"};
    const run = (command, args) => {
        const result = spawnSync(command, args, {cwd: root, env, encoding: "utf8", timeout: 30_000, maxBuffer: 4 * 1024 * 1024});
        assert.ifError(result.error);
        assert.equal(result.status, 0, result.stderr);
        return result.stdout.trim();
    };
    try {
        const files = new Set(["mise.toml", "mise.lock", "Cargo.toml", "tests/Cargo.toml", "tests/mise.toml", "home/mise.toml", "skills/mise.toml",
            ...globSync(["src/**/Cargo.toml", "src/**/mise.toml"],
                {cwd: source, exclude: ["**/target/**"]})]);
        for (const file of files) {
            const destination = join(root, file);
            mkdirSync(dirname(destination), {recursive: true});
            copyFileSync(join(source, file), destination);
        }
        run("git", ["init", "--quiet"]);
        run("git", ["add", "."]);
        const tree = run("git", ["write-tree"]);
        // Fixture-only object creation; no hooks, signing configuration changes, or publication.
        const commit = run("git", ["commit-tree", tree, "-m", "Neutral graph fixture"]);
        run("git", ["update-ref", "HEAD", commit]);
        const all = JSON.parse(run("mise", ["tasks", "graph", "--json"])).projects.map(project => project.id);
        const home = ["custom:home", "cargo:herdr-plugin-contracts", "cargo:dots-repository-tests"];
        const skills = ["custom:skills", ...home];
        for (const [file, expected] of [
            ["src/github/client/src/probe.rs", ["cargo:github-client", "cargo:github-inspect", "cargo:herdr-prs", "cargo:dots-cli", "cargo:herdr-github", "cargo:herdr-review", "cargo:herdr-find", "custom:herdr-deploy", ...skills]],
            ["src/herdr/nav/src/probe.rs", ["cargo:herdr-nav", "cargo:herdr-plugin-contracts", "custom:herdr-deploy"]],
            ["src/herdr/review/core/src/probe.rs", ["cargo:review-core", "cargo:herdr-review", "cargo:herdr-find", "custom:herdr-deploy", ...home]],
            ["src/tasks/src/probe.rs", ["cargo:dots-tasks", "cargo:dots-cli", ...skills]],
            ["src/herdr/tests/src/probe.rs", ["cargo:herdr-plugin-contracts"]],
            ["src/pi/extensions/probe.ts", ["custom:pi"]],
            ["src/claude/codex-agent/hooks/probe.ts", ["custom:codex-agent", ...home]],
            ["home/zsh/.zshrc-probe", home],
            ["skills/herdr-plugin/probe.md", skills],
            ["skills/.local/share/agent-skills/golang/skills/probe.md", skills],
            ["src/herdr/review/editor/lua/probe.lua", ["cargo:herdr-review", "cargo:herdr-find", "custom:herdr-deploy", ...home]],
            ["src/aerospace/probe.py", ["custom:aerospace", ...home]],
            ["src/herdr/deploy/probe.py", ["custom:herdr-deploy"]],
            ["tests/shared-probe.mjs", ["cargo:dots-repository-tests"]],
            ["build-policy.txt", all], ["Cargo.lock", all],
        ]) {
            await t.test(file, () => {
                const changed = join(root, file);
                mkdirSync(dirname(changed), {recursive: true});
                writeFileSync(changed, "neutral fixture\n");
                try {
                    const result = JSON.parse(run("mise", ["run", "--affected", "--affected-untracked", "--affected-json", "//...:check"]));
                    const selected = new Set(result.tasks.flatMap(task => task.projects));
                    assert.deepEqual([...selected].sort(), [...expected].sort());
                } finally { unlinkSync(changed); }
            });
        }
    } finally { rmSync(root, {recursive: true, force: true}); }
});
