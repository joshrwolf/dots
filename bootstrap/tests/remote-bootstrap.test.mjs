import assert from "node:assert/strict";
import {spawnSync} from "node:child_process";
import {copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync} from "node:fs";
import {join} from "node:path";
import {tmpdir} from "node:os";
import {fileURLToPath} from "node:url";
import {test} from "node:test";

const source = fileURLToPath(new URL("../bootstrap/remote/", import.meta.url));

function fixture(t) {
    const root = realpathSync(mkdtempSync(join(tmpdir(), "dots-remote-contract-")));
    t.after(() => rmSync(root, {recursive: true, force: true}));
    const home = join(root, "home");
    const stage = join(root, "stage");
    mkdirSync(home);
    mkdirSync(stage);
    const env = {
        PATH: process.env.PATH, HOME: home, NO_COLOR: "1", MISE_YES: "1",
        XDG_CONFIG_HOME: join(home, ".config"), XDG_DATA_HOME: join(home, ".local/share"),
        XDG_STATE_HOME: join(home, ".local/state"), XDG_CACHE_HOME: join(home, ".cache"),
        MISE_SYSTEM_CONFIG_DIR: join(root, "system"), MISE_GLOBAL_CONFIG_FILE: join(home, "empty.toml"),
        MISE_TRUSTED_CONFIG_PATHS: root, GIT_CONFIG_NOSYSTEM: "1", GIT_CONFIG_GLOBAL: "/dev/null",
        GIT_TERMINAL_PROMPT: "0",
    };
    writeFileSync(env.MISE_GLOBAL_CONFIG_FILE, "");
    const exec = (cwd, command, args) => spawnSync(command, args, {cwd, env, encoding: "utf8", timeout: 30_000});
    const ok = (cwd, command, args) => {
        const result = exec(cwd, command, args);
        assert.equal(result.status, 0, `${command} ${args.join(" ")}\n${result.stdout}\n${result.stderr}\n${result.error ?? ""}`);
        return result.stdout.trim();
    };
    for (const name of [".miserc.toml", "mise.toml", "mise.private.toml"]) {
        copyFileSync(join(source, name), join(stage, name));
    }
    function repository(name, profile) {
        const path = join(root, name);
        mkdirSync(join(path, "bootstrap"), {recursive: true});
        mkdirSync(join(path, "home/example"), {recursive: true});
        writeFileSync(join(path, "home/example", `.${name}`), `${name}\n`);
        writeFileSync(join(path, ".gitignore"), "/private/\n");
        writeFileSync(join(path, "mise.toml"), '[tasks.bootstrap]\nrun = "exit 93"\n');
        copyFileSync(join(source, ".miserc.toml"), join(path, "bootstrap/.miserc.toml"));
        // Exercise the real installer, not a fake that could drift from its
        // preflight, platform and configuration-discovery behavior.
        copyFileSync(join(source, "../mise.toml"), join(path, "bootstrap/mise.toml"));
        writeFileSync(join(path, "bootstrap", `mise.${profile}.toml`), '[vars]\npackages = "example"\n');
        ok(path, "git", ["init", "--quiet"]);
        ok(path, "git", ["add", "."]);
        ok(path, "git", ["-c", "user.name=Bootstrap Test", "-c", "user.email=bootstrap@example.invalid", "-c", "commit.gpgsign=false", "commit", "--quiet", "-m", "Neutral bootstrap fixture"]);
        return {path, ref: ok(path, "git", ["rev-parse", "HEAD"])};
    }
    const publicRepo = repository("public-source", "agents");
    const privateRepo = repository("private-source", "skills");
    const checkout = join(home, "persistent checkout/dots");
    function config(overrides = {}) {
        const vars = {
            checkout, public_url: publicRepo.path, public_ref: publicRepo.ref,
            private_url: privateRepo.path, private_ref: privateRepo.ref, ...overrides,
        };
        writeFileSync(join(stage, "mise.local.toml"), "[vars]\n" + Object.entries(vars)
            .map(([key, value]) => `${key} = ${JSON.stringify(value)}\n`).join(""));
    }
    config();
    const advancePublic = () => {
        writeFileSync(join(publicRepo.path, "home/example/.public-source"), "new source revision\n");
        ok(publicRepo.path, "git", ["add", "."]);
        ok(publicRepo.path, "git", ["-c", "user.name=Bootstrap Test", "-c", "user.email=bootstrap@example.invalid", "-c", "commit.gpgsign=false", "commit", "--quiet", "-m", "Next neutral revision"]);
        return ok(publicRepo.path, "git", ["rev-parse", "HEAD"]);
    };
    return {root, home, stage, checkout, publicRepo, privateRepo, config, advancePublic,
        run: (...args) => exec(stage, "mise", args),
        ok: (...args) => ok(stage, "mise", args)};
}

test("local dry run leaves repositories and installed files untouched", t => {
    const {home, checkout, ok} = fixture(t);
    ok("-E", "private", "bootstrap", "--dry-run");
    assert.ok(!existsSync(checkout));
    assert.ok(!existsSync(join(home, ".public-source")));
    assert.ok(!existsSync(join(home, ".private-source")));
});

test("bootstrap clones pinned source into persistent paths and reapplies without SSH", t => {
    const {home, checkout, stage, ok} = fixture(t);
    ok("bootstrap");
    ok("bootstrap");
    assert.ok(!existsSync(join(checkout, "private")));
    assert.equal(realpathSync(join(home, ".public-source")), join(checkout, "home/example/.public-source"));
    rmSync(stage, {recursive: true});
    assert.equal(readFileSync(join(home, ".public-source"), "utf8"), "public-source\n");
});

test("explicit private selection clones both repositories and survives staging removal", t => {
    const {home, checkout, stage, ok} = fixture(t);
    ok("-E", "private", "bootstrap");
    ok("-E", "private", "bootstrap");
    assert.equal(realpathSync(join(home, ".private-source")), join(checkout, "private/home/example/.private-source"));
    rmSync(stage, {recursive: true});
    assert.equal(readFileSync(join(home, ".public-source"), "utf8"), "public-source\n");
    assert.equal(readFileSync(join(home, ".private-source"), "utf8"), "private-source\n");
});

test("new upstream commits are ignored until the requested pin changes", t => {
    const {home, config, advancePublic, ok} = fixture(t);
    ok("bootstrap");
    const next = advancePublic();
    ok("bootstrap");
    assert.equal(readFileSync(join(home, ".public-source"), "utf8"), "public-source\n");
    config({public_ref: next});
    ok("bootstrap");
    assert.equal(readFileSync(join(home, ".public-source"), "utf8"), "new source revision\n");
});

test("mutable refs fail before repository creation", t => {
    const {checkout, config, run} = fixture(t);
    config({public_ref: "main"});
    const result = run("bootstrap");
    assert.notEqual(result.status, 0);
    assert.match(result.stdout + result.stderr, /full commit SHA/);
    assert.ok(!existsSync(checkout));
});

test("private refs are also pinned before either repository is created", t => {
    const {checkout, config, run} = fixture(t);
    config({private_ref: "main"});
    const result = run("-E", "private", "bootstrap");
    assert.notEqual(result.status, 0);
    assert.match(result.stdout + result.stderr, /private_ref must be a full commit SHA/);
    assert.ok(!existsSync(checkout));
});

test("unknown selections fail instead of reporting an empty successful install", t => {
    const {home, config, run} = fixture(t);
    config({private_profile: "absent"});
    const result = run("-E", "private", "bootstrap");
    assert.notEqual(result.status, 0);
    assert.ok(!existsSync(join(home, ".public-source")));
    assert.ok(!existsSync(join(home, ".private-source")));
});

test("private conflicts stop installation of both selections", t => {
    const {home, run} = fixture(t);
    writeFileSync(join(home, ".private-source"), "unmanaged\n");
    const result = run("-E", "private", "bootstrap");
    assert.notEqual(result.status, 0);
    assert.equal(readFileSync(join(home, ".private-source"), "utf8"), "unmanaged\n");
    assert.ok(!existsSync(join(home, ".public-source")));
});

test("bootstrap preserves dirty checkouts rather than resetting them", t => {
    const {checkout, run, ok} = fixture(t);
    ok("bootstrap");
    writeFileSync(join(checkout, "home/example/.public-source"), "local work\n");
    const result = run("bootstrap");
    assert.notEqual(result.status, 0);
    assert.equal(readFileSync(join(checkout, "home/example/.public-source"), "utf8"), "local work\n");
});
