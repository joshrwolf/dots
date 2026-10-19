import assert from "node:assert/strict";
import {createHash} from "node:crypto";
import {cpSync, existsSync, lstatSync, mkdirSync, readFileSync, readlinkSync, realpathSync, renameSync, rmSync, symlinkSync} from "node:fs";
import {homedir} from "node:os";
import {dirname, join, relative, resolve} from "node:path";
import {fileURLToPath} from "node:url";
import {SettingsManager} from "@earendil-works/pi-coding-agent";
import {root, run, runtime, verifyRelease, verifyRuntime} from "./artifact.mjs";

// Public SDK API owns settings persistence and preserves unrelated settings.
const specification = entry => typeof entry === "string" ? entry : entry?.source;
function localPath(spec, agent) {
    if (typeof spec !== "string" || /^(npm:|git:|https?:|ssh:|git\/\/)/.test(spec)) return undefined;
    return resolve(agent, spec.startsWith("~/") ? join(homedir(), spec.slice(2)) : spec);
}
export function registrationPlan(entries, agent, current, name, replaceSource) {
    assert.ok(Array.isArray(entries), "Pi packages must be an array");
    let found = false;
    const result = entries.map(entry => {
        const spec = specification(entry);
        assert.equal(typeof spec, "string", "invalid package registration");
        const path = localPath(spec, agent);
        let identity;
        if (spec.startsWith("npm:")) identity = spec.slice(4).replace(/@[^/@]+$/, "");
        else if (path && existsSync(join(path, "package.json"))) identity = JSON.parse(readFileSync(join(path, "package.json"), "utf8")).name;
        const allowed = path === current || (replaceSource && path === resolve(replaceSource));
        if (allowed && identity) assert.equal(identity, name, "replacement source belongs to another package");
        if (!allowed && identity !== name) return entry;
        assert.ok(allowed, `Another copy of ${name} is registered: ${spec}; explicitly pass --replace-source for a local source`);
        assert.ok(!found, "duplicate companion registration");
        found = true;
        return typeof entry === "string" ? current : {...entry, source: current};
    });
    if (replaceSource) assert.ok(found, "replacement source is not registered");
    if (!found) result.push(current);
    return result;
}
export function upstreamPlan(pins, entries, agent) {
    assert.ok(Array.isArray(pins) && Array.isArray(entries), "pins and registrations must be arrays");
    const desired = new Map();
    for (const pin of pins) {
        const match = typeof pin === "string" && /^npm:((?:@[a-z0-9._-]+\/)?[a-z0-9._-]+)@\d+\.\d+\.\d+(?:-[a-zA-Z0-9.-]+)?$/.exec(pin);
        assert.ok(match, "upstream packages require exact npm versions");
        assert.ok(!desired.has(match[1]), "duplicate upstream package");
        desired.set(match[1], pin);
    }
    const registered = new Set();
    for (const entry of entries) {
        const spec = specification(entry);
        assert.equal(typeof spec, "string", "invalid package registration");
        const path = localPath(spec, agent);
        const name = spec.startsWith("npm:") ? spec.slice(4).replace(/@[^/@]+$/, "")
            : path && existsSync(join(path, "package.json"))
                ? JSON.parse(readFileSync(join(path, "package.json"), "utf8")).name
                : spec.replace(/\/$/, "").split("/").at(-1).split("@")[0].replace(/\.git$/, "");
        if (!desired.has(name)) continue;
        assert.equal(spec, desired.get(name), `another copy/version of ${name} is registered; remove it explicitly first`);
        assert.ok(!registered.has(name), "duplicate upstream registration");
        registered.add(name);
    }
    return [...desired].map(([name, pin]) => [registered.has(name) ? "update" : "install", "--no-approve", pin]);
}

export function installUpstream(agent) {
    outsideCheckout(agent);
    const config = JSON.parse(readFileSync(join(root, "pi-packages.json"), "utf8"));
    assert.deepEqual(Object.keys(config), ["npm"], "expected only an npm package list");
    const settings = SettingsManager.create(agent, agent, {projectTrusted: false});
    assert.deepEqual(settings.drainErrors(), [], "cannot read Pi settings");
    const actions = upstreamPlan(config.npm, settings.getGlobalSettings().packages ?? [], agent);
    mkdirSync(agent, {recursive: true, mode: 0o700});
    for (const action of actions) run(process.env.PI_BIN ?? "pi", action, agent);
    console.log("Upstream packages reconciled through Pi; no session was reloaded.");
}

function outsideCheckout(...paths) {
    for (const path of paths) {
        let existing = resolve(path);
        while (!existsSync(existing)) existing = dirname(existing);
        const inside = relative(root, realpathSync(existing));
        assert.ok(inside === ".." || inside.startsWith("../"), "agent state resolves into the checkout; unfold the Stow directory before installation");
    }
}
export async function install(agent, replaceSource, persist) {
    const base = join(agent, "dots/packages/pi-dots");
    const releases = join(base, "releases");
    outsideCheckout(agent, base, releases, join(agent, "settings.json"));
    const ready = verifyRuntime();
    const manifestText = readFileSync(join(runtime, "package.json"), "utf8");
    assert.equal(createHash("sha256").update(manifestText).digest("hex"),
        ready.files.find(([path]) => path === "package.json")?.[1], "prepared manifest changed during installation");
    const manifest = JSON.parse(manifestText);
    assert.equal(run(process.env.PI_BIN ?? "pi", ["--version"], process.cwd()).trim(), manifest.devDependencies["@earendil-works/pi-coding-agent"], "installed Pi must match the checked development SDK");
    const current = join(base, "current");
    mkdirSync(base, {recursive: true, mode: 0o700});
    const lock = join(base, "install.lock");
    mkdirSync(lock); // Fail rather than overlap another installer or guess a stale owner's identity.
    const temporary = join(base, "current.next");
    let old;
    let published = false;
    try {
        const settings = SettingsManager.create(agent, agent, {projectTrusted: false});
        const errors = settings.drainErrors();
        assert.deepEqual(errors, [], "cannot read Pi settings");
        const before = settings.getGlobalSettings().packages ?? [];
        registrationPlan(before, agent, current, manifest.name, replaceSource);
        try {
            assert.ok(lstatSync(current).isSymbolicLink(), "refusing to replace a non-symlink runtime");
            old = readlinkSync(current);
        } catch (error) { if (error.code !== "ENOENT") throw error; }
        try {
            assert.ok(lstatSync(join(base, "previous")).isSymbolicLink(), "refusing to replace a non-symlink previous runtime");
        } catch (error) { if (error.code !== "ENOENT") throw error; }
        const digest = createHash("sha256").update(JSON.stringify(ready)).digest("hex");
        mkdirSync(releases, {recursive: true});
        const release = join(releases, digest);
        if (!existsSync(release)) {
            const stage = join(releases, digest + ".next");
            try {
                cpSync(runtime, stage, {recursive: true, verbatimSymlinks: true, errorOnExist: true, force: false});
                assert.deepEqual(verifyRuntime(stage), ready, "prepared runtime changed during installation");
                renameSync(stage, release);
            } finally { rmSync(stage, {recursive: true, force: true}); }
        }
        assert.deepEqual(verifyRuntime(release), ready, "release does not match its content identity");
        // Refresh just before persistence. Cutover requires quiescent package writers;
        // the SDK merges unrelated settings but packages itself is one array field.
        await settings.reload();
        assert.deepEqual(settings.drainErrors(), [], "cannot reload Pi settings");
        const next = registrationPlan(settings.getGlobalSettings().packages ?? [], agent, current, manifest.name, replaceSource);
        symlinkSync(release, temporary);
        renameSync(temporary, current);
        published = true;
        if (persist) await persist(settings, next);
        else {
            settings.setPackages(next);
            await settings.flush();
            assert.deepEqual(settings.drainErrors(), [], "Pi settings persistence failed");
        }
        if (old !== undefined && old !== release) {
            symlinkSync(old, temporary);
            renameSync(temporary, join(base, "previous"));
        }
        console.log("Prepared companion activated. Restart Pi when ready; no session was reloaded.");
    } catch (error) {
        if (published) {
            rmSync(temporary, {force: true});
            if (old !== undefined) { symlinkSync(old, temporary); renameSync(temporary, current); }
            else rmSync(current, {force: true});
        }
        throw error;
    } finally {
        rmSync(temporary, {force: true});
        rmSync(lock, {recursive: true});
    }
}
export function rollback(agent) {
    const base = join(agent, "dots/packages/pi-dots");
    outsideCheckout(agent, base, join(base, "releases"));
    const lock = join(base, "install.lock");
    mkdirSync(lock);
    const temporary = join(base, "current.next");
    try {
        const current = join(base, "current");
        const previous = join(base, "previous");
        const target = readlinkSync(previous);
        const directory = resolve(base, target);
        outsideCheckout(directory);
        verifyRelease(directory);
        const manifest = JSON.parse(readFileSync(join(directory, "package.json"), "utf8"));
        assert.equal(run(process.env.PI_BIN ?? "pi", ["--version"], process.cwd()).trim(), manifest.devDependencies["@earendil-works/pi-coding-agent"], "rollback release requires its matching Pi host");
        const old = readlinkSync(current);
        symlinkSync(target, temporary);
        renameSync(temporary, current);
        symlinkSync(old, temporary);
        renameSync(temporary, previous);
        console.log("Previous companion activated; restart Pi when ready.");
    } finally {
        rmSync(temporary, {force: true});
        rmSync(lock, {recursive: true});
    }
}
if (process.argv[1] === fileURLToPath(import.meta.url)) {
    const args = process.argv.slice(2);
    assert.ok(args.length === 0 || (args.length === 1 && ["--rollback", "--upstream"].includes(args[0])) || (args.length === 2 && args[0] === "--replace-source"), "usage: install.mjs [--replace-source PATH | --rollback | --upstream]");
    const agent = resolve(process.env.PI_CODING_AGENT_DIR ?? join(homedir(), ".pi/agent"));
    if (args[0] === "--upstream") installUpstream(agent);
    else if (args[0] === "--rollback") rollback(agent);
    else await install(agent, args[1]);
}
