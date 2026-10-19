import assert from "node:assert/strict";
import fs from "node:fs";
import {syncBuiltinESMExports} from "node:module";
import {cpSync, mkdtempSync, mkdirSync, readFileSync, readlinkSync, rmSync, symlinkSync, writeFileSync} from "node:fs";
import {tmpdir} from "node:os";
import {join} from "node:path";
import {test} from "node:test";
import {install, registrationPlan, rollback} from "../../install/index.mjs";
import {root, runtime} from "../../install/artifact.mjs";

function scratch(t) {
    const agent = mkdtempSync(join(tmpdir(), "dots-install-"));
    t.after(() => rmSync(agent, {recursive: true, force: true}));
    return agent;
}
const settings = agent => JSON.parse(readFileSync(join(agent, "settings.json"), "utf8"));
const current = agent => join(agent, "dots/packages/pi-dots/current");

test("agent state must not be installed into the checkout", async () => {
    await assert.rejects(install(join(runtime, "agent-state")), /agent state resolves into the checkout/);
});

test("redirected state descendants fail before writing into source", async t => {
    for (const suffix of ["dots", "dots/packages/pi-dots/releases"]) {
        const agent = scratch(t);
        const target = mkdtempSync(join(root, "dist/pi-isolation-"));
        t.after(() => rmSync(target, {recursive: true, force: true}));
        const path = join(agent, suffix);
        mkdirSync(join(path, ".."), {recursive: true});
        symlinkSync(target, path);
        await assert.rejects(install(agent), /agent state resolves into the checkout/);
        assert.throws(() => rollback(agent), /agent state resolves into the checkout/);
        assert.deepEqual(fs.readdirSync(target), [], "isolation must fail before mutation");
    }
});

test("post-persistence bookkeeping failure restores the original pointer", async t => {
    const agent = scratch(t);
    const base = join(agent, "dots/packages/pi-dots");
    mkdirSync(base, {recursive: true});
    symlinkSync("old-release", current(agent));
    writeFileSync(join(agent, "settings.json"), JSON.stringify({packages: [current(agent)]}));
    const rename = fs.renameSync;
    const mocked = t.mock.method(fs, "renameSync", (source, destination) => {
        if (destination === join(base, "previous")) throw new Error("injected previous publication failure");
        return rename(source, destination);
    });
    syncBuiltinESMExports();
    try {
        await assert.rejects(install(agent), /injected previous publication failure/);
        assert.equal(readlinkSync(current(agent)), "old-release");
        assert.ok(!fs.existsSync(join(base, "current.next")));
    } finally {
        mocked.mock.restore();
        syncBuiltinESMExports();
    }
});

test("real SDK registration is idempotent and preserves unrelated settings", async t => {
    const agent = scratch(t);
    writeFileSync(join(agent, "settings.json"), JSON.stringify({theme: "example", packages: ["npm:unrelated@1.0.0"]}));
    await install(agent);
    const before = settings(agent);
    const release = readlinkSync(current(agent));
    await install(agent);
    assert.deepEqual(settings(agent), before);
    assert.equal(readlinkSync(current(agent)), release);
    assert.equal(before.theme, "example");
    assert.deepEqual(before.packages, ["npm:unrelated@1.0.0", current(agent)]);
});

test("explicit source migration preserves every resource filter", async t => {
    const agent = scratch(t);
    const old = join(agent, "old-source");
    mkdirSync(old);
    writeFileSync(join(old, "package.json"), JSON.stringify({name: "@joshrwolf/pi-dots"}));
    const entry = {source: old, extensions: ["!extensions/example.ts"], themes: [], skills: [], autoload: false};
    writeFileSync(join(agent, "settings.json"), JSON.stringify({packages: [entry]}));
    await install(agent, old);
    assert.deepEqual(settings(agent).packages, [{...entry, source: current(agent)}]);
});

test("conflicting npm registration fails before activation", async t => {
    const agent = scratch(t);
    const text = JSON.stringify({packages: ["npm:@joshrwolf/pi-dots@0.1.0"]});
    writeFileSync(join(agent, "settings.json"), text);
    await assert.rejects(install(agent), /Another copy/);
    assert.equal(readFileSync(join(agent, "settings.json"), "utf8"), text);
    assert.throws(() => readlinkSync(current(agent)), /ENOENT/);
});

test("explicit replacement refuses another package identity", async t => {
    const agent = scratch(t);
    const old = join(agent, "other-package");
    mkdirSync(old);
    writeFileSync(join(old, "package.json"), JSON.stringify({name: "@example/other"}));
    writeFileSync(join(agent, "settings.json"), JSON.stringify({packages: [old]}));
    await assert.rejects(install(agent, old), /another package/);
    assert.deepEqual(settings(agent).packages, [old]);
});

test("persistence failure restores the previous runtime pointer", async t => {
    const agent = scratch(t);
    const base = join(agent, "dots/packages/pi-dots");
    mkdirSync(base, {recursive: true});
    symlinkSync("previous-release", current(agent));
    await assert.rejects(install(agent, undefined, async () => { throw new Error("injected persistence failure"); }), /injected/);
    assert.equal(readlinkSync(current(agent)), "previous-release");
});

test("first-install persistence failure leaves no active runtime", async t => {
    const agent = scratch(t);
    await assert.rejects(install(agent, undefined, async () => { throw new Error("injected"); }), /injected/);
    assert.throws(() => readlinkSync(current(agent)), /ENOENT/);
});

test("installer refuses an existing directory and overlapping installer", async t => {
    const agent = scratch(t);
    mkdirSync(current(agent), {recursive: true});
    await assert.rejects(install(agent), /non-symlink/);
    mkdirSync(join(agent, "dots/packages/pi-dots/install.lock"));
    await assert.rejects(install(agent), /EEXIST/);
});

test("a different valid snapshot cannot be published under the selected release identity", async t => {
    const agent = scratch(t);
    const copy = fs.cpSync;
    const mocked = t.mock.method(fs, "cpSync", (source, destination, options) => {
        copy(source, destination, options);
        if (!String(destination).endsWith(".next")) return;
        const path = join(destination, ".dots-ready.json");
        const ready = JSON.parse(readFileSync(path, "utf8"));
        writeFileSync(path, JSON.stringify({...ready, snapshot: "different preparation"}));
    });
    syncBuiltinESMExports();
    try {
        await assert.rejects(install(agent), /prepared runtime changed during installation/);
        assert.throws(() => readlinkSync(current(agent)), /ENOENT/);
    } finally {
        mocked.mock.restore();
        syncBuiltinESMExports();
    }
});

test("explicit rollback verifies the prior release and swaps complete pointers", async t => {
    const agent = scratch(t);
    await install(agent);
    const release = readlinkSync(current(agent));
    const base = join(agent, "dots/packages/pi-dots");
    const prior = join(base, "releases/prior-fixture");
    cpSync(release, prior, {recursive: true, verbatimSymlinks: true});
    symlinkSync(prior, join(base, "previous"));
    rollback(agent);
    assert.equal(readlinkSync(current(agent)), prior);
    assert.equal(readlinkSync(join(base, "previous")), release);
    writeFileSync(join(release, "package.json"), "corrupt");
    assert.throws(() => rollback(agent), /runtime changed/);
    assert.equal(readlinkSync(current(agent)), prior);
});

test("registration validation rejects duplicates and nonexistent replacements", () => {
    assert.throws(() => registrationPlan(["/agent/current", "/agent/current"], "/agent", "/agent/current", "example"), /duplicate/);
    assert.throws(() => registrationPlan([], "/agent", "/agent/current", "example", "/missing"), /not registered/);
});
