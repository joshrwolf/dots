import assert from "node:assert/strict";
import {spawnSync} from "node:child_process";
import {copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readlinkSync, realpathSync, rmSync, symlinkSync, writeFileSync} from "node:fs";
import {join} from "node:path";
import {arch, platform, tmpdir} from "node:os";
import {fileURLToPath} from "node:url";
import {test} from "node:test";

const source = fileURLToPath(new URL("../bootstrap/", import.meta.url));

function fixture(t) {
    const root = realpathSync(mkdtempSync(join(tmpdir(), "dots-bootstrap-")));
    t.after(() => rmSync(root, {recursive: true, force: true}));
    const home = join(root, "home");
    const checkout = join(root, "checkout");
    const setup = join(checkout, "bootstrap");
    mkdirSync(home);
    mkdirSync(checkout);
    mkdirSync(setup);
    for (const file of [".miserc.toml", "mise.toml", "mise.agents.toml", "mise.desktop.toml"]) {
        copyFileSync(join(source, file), join(setup, file));
    }
    writeFileSync(join(checkout, "mise.toml"), `
monorepo_root = true
[monorepo]
config_roots = ["bootstrap", "private/*"]
[vars]
base_stow_packages = "codex skills zsh"
[tools]
node = "24"
[env]
DOTS_PARENT_CONFIG_LOADED = "yes"
[tasks.bootstrap]
run = "touch '${join(home, "parent-ran")}'"
`);
    const env = {
        PATH: process.env.PATH,
        HOME: home,
        XDG_CONFIG_HOME: join(home, ".config"),
        XDG_DATA_HOME: join(home, ".local/share"),
        XDG_STATE_HOME: join(home, ".local/state"),
        XDG_CACHE_HOME: join(home, ".cache"),
        MISE_GLOBAL_CONFIG_FILE: join(home, "empty.toml"),
        MISE_SYSTEM_CONFIG_DIR: join(root, "system"),
        MISE_TRUSTED_CONFIG_PATHS: root,
        MISE_YES: "1",
        NO_COLOR: "1",
    };
    writeFileSync(env.MISE_GLOBAL_CONFIG_FILE, "");
    const run = (...args) => spawnSync("mise", args, {
        cwd: setup, env, encoding: "utf8", timeout: 30_000,
    });
    const ok = (...args) => {
        const result = run(...args);
        assert.equal(result.status, 0, `${args.join(" ")}\n${result.stdout}\n${result.stderr}\n${result.error ?? ""}`);
        return result.stdout;
    };
    return {root, home, checkout, setup, env, run, ok, globalConfig: env.MISE_GLOBAL_CONFIG_FILE};
}

function stowFixture(t) {
    const f = fixture(t);
    // Use the real instruction package and neutral skill fixtures. Never copy
    // ignored private skills or mutable machine state into a test home.
    mkdirSync(join(f.checkout, "home/codex/.codex"), {recursive: true});
    copyFileSync(join(source, "../home/codex/.codex/AGENTS.md"), join(f.checkout, "home/codex/.codex/AGENTS.md"));
    mkdirSync(join(f.checkout, "home/codex/.claude/rules"), {recursive: true});
    symlinkSync(readlinkSync(join(source, "../home/codex/.claude/rules/personal.md")),
        join(f.checkout, "home/codex/.claude/rules/personal.md"));
    for (const name of ["golang-example", "idiomatic-rust", "local-example", "herdr-example", "dots"]) {
        const canonical = join(f.checkout, "skills", name);
        mkdirSync(canonical, {recursive: true});
        writeFileSync(join(canonical, "SKILL.md"), `# ${name}\n`);
        for (const agent of [".agents", ".claude"]) {
            const discovery = join(f.checkout, "home/skills", agent, "skills");
            mkdirSync(discovery, {recursive: true});
            symlinkSync(`../../../../skills/${name}`, join(discovery, name));
        }
    }
    mkdirSync(join(f.checkout, "home/pi/.pi/agent/themes"), {recursive: true});
    copyFileSync(join(source, "../home/pi/.pi/agent/AGENTS.md"), join(f.checkout, "home/pi/.pi/agent/AGENTS.md"));
    symlinkSync(readlinkSync(join(source, "../home/pi/.pi/agent/themes/dots.json")),
        join(f.checkout, "home/pi/.pi/agent/themes/dots.json"));
    for (const name of ["aerospace", "brew", "ghostty", "theme", "zsh"]) {
        mkdirSync(join(f.checkout, "home", name));
    }
    writeFileSync(join(f.checkout, "home/zsh/.zshrc"), "# desktop shell\n");
    return f;
}

test("portable agents install only selected configuration and preserve the host", t => {
    const {home, checkout, ok} = stowFixture(t);
    writeFileSync(join(home, ".zshrc"), "# host shell\n");
    mkdirSync(join(home, ".codex"));
    writeFileSync(join(home, ".codex/config.toml"), "# host settings\n");
    ok("-E", "agents", "run", "preview");
    ok("-E", "agents", "bootstrap", "--dry-run");
    assert.ok(!existsSync(join(home, ".codex/AGENTS.md")));
    ok("-E", "agents", "bootstrap");
    ok("-E", "agents", "bootstrap");
    for (const agent of [".agents", ".claude"]) {
        for (const name of ["golang-example", "idiomatic-rust"]) {
            assert.equal(realpathSync(join(home, agent, "skills", name)), join(checkout, "skills", name));
        }
        for (const name of ["local-example", "herdr-example", "dots"]) {
            assert.ok(!existsSync(join(home, agent, "skills", name)), `${name} must be excluded`);
        }
    }
    assert.ok(existsSync(join(home, ".codex/AGENTS.md")));
    assert.ok(existsSync(join(home, ".claude/rules/personal.md")));
    assert.ok(!existsSync(join(home, ".claude/CLAUDE.md")));
    assert.ok(existsSync(join(home, ".pi/agent/AGENTS.md")));
    assert.ok(!existsSync(join(home, ".pi/agent/themes")));
    assert.equal(readFileSync(join(home, ".zshrc"), "utf8"), "# host shell\n");
    assert.equal(readFileSync(join(home, ".codex/config.toml"), "utf8"), "# host settings\n");
    assert.deepEqual(JSON.parse(ok("-E", "agents", "ls", "--current", "--json")), {});
});

test("agent preflight rejects conflicts before installing any selected package", t => {
    const {home, run} = stowFixture(t);
    mkdirSync(join(home, ".agents/skills/idiomatic-rust"), {recursive: true});
    writeFileSync(join(home, ".agents/skills/idiomatic-rust/SKILL.md"), "keep me\n");
    const preview = run("-E", "agents", "run", "preview");
    assert.notEqual(preview.status, 0, preview.stdout + preview.stderr);
    const result = run("-E", "agents", "bootstrap");
    assert.notEqual(result.status, 0, result.stdout + result.stderr);
    assert.equal(readFileSync(join(home, ".agents/skills/idiomatic-rust/SKILL.md"), "utf8"), "keep me\n");
    assert.ok(!existsSync(join(home, ".codex/AGENTS.md")));
});

test("desktop mirrors the root package inventory without loading development tools", t => {
    const rootConfig = readFileSync(join(source, "../mise.toml"), "utf8");
    const desktopConfig = readFileSync(join(source, "mise.desktop.toml"), "utf8");
    assert.equal(desktopConfig.match(/^packages = "([^"]+)"/m)?.[1],
        rootConfig.match(/^base_stow_packages = "([^"]+)"/m)?.[1]);
    const {home, run, ok} = stowFixture(t);
    assert.deepEqual(JSON.parse(ok("-E", "desktop", "ls", "--current", "--json")), {});
    if (platform() === "darwin" && arch() === "arm64") {
        ok("-E", "desktop", "bootstrap");
        ok("-E", "desktop", "bootstrap");
        assert.equal(readFileSync(join(home, ".zshrc"), "utf8"), "# desktop shell\n");
        assert.ok(existsSync(join(home, ".agents/skills/dots/SKILL.md")));
    } else {
        const result = run("-E", "desktop", "bootstrap");
        assert.notEqual(result.status, 0);
        assert.ok(!existsSync(join(home, ".zshrc")));
    }
});

test("private configuration stays opt-in and composes without owning public files", t => {
    const {home, checkout, env, ok} = stowFixture(t);
    const privateRoot = join(checkout, "private");
    const privateSetup = join(privateRoot, "bootstrap");
    const privateHome = join(privateRoot, "home/skills");
    mkdirSync(privateSetup, {recursive: true});
    copyFileSync(join(source, ".miserc.toml"), join(privateSetup, ".miserc.toml"));
    writeFileSync(join(privateSetup, "mise.toml"), `
[tasks.bootstrap]
run = [
  'stow --dir="{{ config_root }}/../home" --target="$HOME" --no-folding --simulate --restow skills',
  'stow --dir="{{ config_root }}/../home" --target="$HOME" --no-folding --restow skills',
]
`);
    const skill = join(privateRoot, "skills/local-example");
    mkdirSync(skill, {recursive: true});
    writeFileSync(join(skill, "SKILL.md"), "# Private example\n");
    for (const agent of [".agents", ".claude"]) {
        const links = join(privateHome, agent, "skills");
        mkdirSync(links, {recursive: true});
        symlinkSync("../../../../skills/local-example", join(links, "local-example"));
    }
    ok("-E", "agents", "bootstrap");
    assert.ok(!existsSync(join(home, ".agents/skills/local-example")));
    const publicInstructions = realpathSync(join(home, ".codex/AGENTS.md"));
    const privateRun = (...args) => spawnSync("mise", args, {
        cwd: privateSetup, env, encoding: "utf8", timeout: 30_000,
    });
    const tools = privateRun("ls", "--current", "--json");
    assert.equal(tools.status, 0, tools.stderr);
    assert.deepEqual(JSON.parse(tools.stdout), {});
    for (let n = 0; n < 2; n++) {
        const applied = privateRun("bootstrap");
        assert.equal(applied.status, 0, applied.stderr);
    }
    for (const agent of [".agents", ".claude"]) {
        assert.equal(realpathSync(join(home, agent, "skills/local-example")), skill);
    }
    ok("-E", "agents", "bootstrap");
    assert.equal(realpathSync(join(home, ".codex/AGENTS.md")), publicInstructions);
    assert.equal(realpathSync(join(home, ".agents/skills/local-example")), skill);
});

test("public Git ignores the independent private checkout", () => {
    const result = spawnSync("git", ["check-ignore", "--no-index", "private/example/mise.toml"], {
        cwd: join(source, ".."), encoding: "utf8", timeout: 10_000,
    });
    assert.equal(result.status, 0, result.stderr);
    assert.equal(result.stdout.trim(), "private/example/mise.toml");
});

test("machine setup excludes ancestor development tools and tasks", t => {
    const {home, ok} = fixture(t);
    assert.deepEqual(JSON.parse(ok("ls", "--current", "--json")), {});
    assert.ok(!ok("env", "--json").includes("DOTS_PARENT_CONFIG_LOADED"));
    ok("bootstrap", "--dry-run");
    ok("bootstrap");
    ok("bootstrap");
    assert.ok(!existsSync(join(home, "parent-ran")));
});

test("machine setup retains explicit global configuration", t => {
    const {globalConfig, ok} = fixture(t);
    writeFileSync(globalConfig, '[env]\nDOTS_GLOBAL_CONFIG_LOADED = "yes"\n');
    const env = JSON.parse(ok("env", "--json"));
    assert.equal(env.DOTS_GLOBAL_CONFIG_LOADED, "yes");
    assert.equal(env.DOTS_PARENT_CONFIG_LOADED, undefined);
});

test("explicit setup modules preserve existing files and can be reapplied", t => {
    const {home, setup, run, ok} = fixture(t);
    const target = join(home, ".config/example/settings");
    mkdirSync(join(setup, "payload"));
    writeFileSync(join(setup, "payload/settings"), "managed configuration\n");
    writeFileSync(join(setup, "mise.example.toml"), `
[dotfiles]
"~/.config/example/settings" = "payload/settings"
`);
    ok("bootstrap");
    assert.ok(!existsSync(target), "unselected modules must not install files");
    ok("-E", "example", "bootstrap", "--dry-run");
    assert.ok(!existsSync(target), "preview must not install files");
    mkdirSync(join(home, ".config/example"), {recursive: true});
    writeFileSync(target, "existing configuration\n");
    const conflict = run("-E", "example", "bootstrap");
    assert.notEqual(conflict.status, 0, "conflicting user files must stop setup");
    assert.equal(readFileSync(target, "utf8"), "existing configuration\n");
    rmSync(target);
    ok("-E", "example", "bootstrap");
    ok("-E", "example", "bootstrap");
    assert.equal(readFileSync(target, "utf8"), "managed configuration\n");
});
