import assert from "node:assert/strict";
import {existsSync, readdirSync, realpathSync} from "node:fs";
import {join} from "node:path";
import {fileURLToPath} from "node:url";
import {test} from "node:test";

const root = fileURLToPath(new URL("../", import.meta.url));
const shared = join(root, "home/skills/.agents/skills");
const claude = join(root, "home/skills/.claude/skills");

test("shared skills have one canonical definition and a Claude discovery alias", () => {
    for (const name of readdirSync(shared)) {
        assert.ok(existsSync(join(shared, name, "SKILL.md")), `${name}: missing canonical skill`);
        assert.equal(realpathSync(join(claude, name)), realpathSync(join(shared, name)), `${name}: wrong Claude alias`);
    }
});

test("authored skills have discovery links without copying their definitions", () => {
    for (const entry of readdirSync(join(root, "skills"), {withFileTypes: true})) {
        if (!entry.isDirectory() || entry.name.startsWith(".")) continue;
        assert.equal(realpathSync(join(shared, entry.name)), realpathSync(join(root, "skills", entry.name)));
    }
});

test("imported skills remain discoverable after a subtree update", () => {
    const source = join(root, "skills/.local/share/agent-skills/golang/skills");
    for (const name of readdirSync(source)) {
        if (!existsSync(join(source, name, "SKILL.md"))) continue;
        assert.equal(realpathSync(join(shared, name)), realpathSync(join(source, name)), `${name}: add its shared discovery alias`);
    }
});

test("Claude-only discovery entries must be actual plugins", () => {
    for (const name of readdirSync(claude)) {
        if (existsSync(join(shared, name))) continue;
        assert.ok(existsSync(join(claude, name, ".claude-plugin/plugin.json")), `${name}: stale discovery alias`);
    }
});
