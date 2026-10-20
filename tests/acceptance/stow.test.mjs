import assert from "node:assert/strict";
import {existsSync, lstatSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, realpathSync, rmSync, writeFileSync} from "node:fs";
import {homedir, tmpdir} from "node:os";
import {join} from "node:path";
import {spawnSync} from "node:child_process";
import {fileURLToPath} from "node:url";
import {test} from "node:test";

const root = fileURLToPath(new URL("../../", import.meta.url));
const available = spawnSync("stow", ["--version"]).status === 0;

test("native Stow tasks preserve runtime state and reject unmanaged files", {skip: !available}, () => {
    const home = mkdtempSync(join(tmpdir(), "dots-stow-"));
    const env = {...process.env, HOME: home,
        MISE_DATA_DIR: process.env.MISE_DATA_DIR ?? join(homedir(), ".local/share/mise"),
        MISE_CACHE_DIR: process.env.MISE_CACHE_DIR ?? join(homedir(), ".cache/mise")};
    const run = task => {
        const result = spawnSync("mise", ["run", task], {cwd: root, env, encoding: "utf8", timeout: 30_000});
        assert.ifError(result.error);
        return result;
    };
    const success = task => {
        const result = run(task);
        assert.equal(result.status, 0, result.stderr);
    };
    try {
        const settings = join(home, ".pi/agent/settings.json");
        mkdirSync(join(home, ".pi/agent"), {recursive: true});
        writeFileSync(settings, '{"theme":"example"}');
        success("install");
        success("install");
        assert.ok(lstatSync(join(home, ".zshrc")).isSymbolicLink());
        assert.ok(!lstatSync(join(home, ".agents")).isSymbolicLink());
        for (const skills of [".agents/skills", ".claude/skills"]) {
            assert.ok(lstatSync(join(home, skills)).isSymbolicLink(), skills);
            assert.equal(realpathSync(join(home, skills)), realpathSync(join(root, "skills")), skills);
            assert.ok(existsSync(join(home, skills, "golang-testing/SKILL.md")), skills);
        }
        assert.ok(existsSync(join(home, ".claude/skills/codex-agent/.claude-plugin/plugin.json")));
        assert.ok(!lstatSync(join(home, ".pi/agent")).isSymbolicLink());
        assert.ok(existsSync(join(home, ".claude/rules/personal.md")));
        assert.ok(!existsSync(join(home, ".claude/CLAUDE.md")));
        success("unstow");
        assert.ok(!existsSync(join(home, ".zshrc")));
        mkdirSync(join(home, ".claude/skills/unmanaged"), {recursive: true});
        assert.notEqual(run("install").status, 0);
        assert.deepEqual(readdirSync(join(home, ".claude/skills")), ["unmanaged"]);
        assert.ok(!existsSync(join(home, ".zshrc")));
        rmSync(join(home, ".claude/skills"), {recursive: true});
        assert.equal(readFileSync(settings, "utf8"), '{"theme":"example"}');
        writeFileSync(join(home, ".zshrc"), "unmanaged configuration\n");
        assert.notEqual(run("install").status, 0);
        assert.equal(readFileSync(join(home, ".zshrc"), "utf8"), "unmanaged configuration\n");
        assert.equal(readFileSync(settings, "utf8"), '{"theme":"example"}');
    } finally { rmSync(home, {recursive: true, force: true}); }
});
