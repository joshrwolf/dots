import assert from "node:assert/strict";
import {createHash} from "node:crypto";
import {cpSync, existsSync, lstatSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync} from "node:fs";
import {homedir} from "node:os";
import {join} from "node:path";
import {spawnSync} from "node:child_process";
import {fileURLToPath} from "node:url";
import {test} from "node:test";
import {copyCheckout} from "../support/checkout.mjs";

const root = fileURLToPath(new URL("../../", import.meta.url));
const staging = () => Object.fromEntries([
    "home/cli/.local/bin/dots", "home/herdr/.local/bin/herdr-issue", "home/herdr/.local/bin/herdr-review",
].map(path => [path, existsSync(join(root, path)) ? {
    hash: createHash("sha256").update(readFileSync(join(root, path))).digest("hex"),
    mode: lstatSync(join(root, path)).mode,
} : null]));

// Both source staging and HOME are disposable: testing with only a temporary
// HOME would still replace binaries linked from the real user's Stow packages.
test("native installation tasks preserve ownership and isolate runtime state", () => {
    const before = staging();
    const temporary = mkdtempSync("/tmp/dci-"); // Herdr's Unix socket has a short path limit.
    const source = join(temporary, "source"), home = join(temporary, "home");
    const env = {...Object.fromEntries(Object.entries(process.env).filter(([key]) => !key.startsWith("HERDR_"))),
        HOME: home, XDG_CONFIG_HOME: join(home, ".config"), XDG_DATA_HOME: join(home, ".local/share"),
        XDG_STATE_HOME: join(home, ".local/state"), XDG_CACHE_HOME: join(home, ".cache"), XDG_RUNTIME_DIR: join(home, ".runtime"),
        MISE_TRUSTED_CONFIG_PATHS: source,
        MISE_DATA_DIR: process.env.MISE_DATA_DIR ?? join(homedir(), ".local/share/mise"),
        MISE_CACHE_DIR: process.env.MISE_CACHE_DIR ?? join(homedir(), ".cache/mise"),
        CARGO_HOME: process.env.CARGO_HOME ?? join(homedir(), ".cargo"),
        RUSTUP_HOME: process.env.RUSTUP_HOME ?? join(homedir(), ".rustup"),
        CARGO_TARGET_DIR: process.env.CARGO_TARGET_DIR ?? join(root, "target"),
    };
    const run = (command, args, cwd = source) => {
        const result = spawnSync(command, args, {cwd, env, encoding: "utf8", timeout: 180_000, maxBuffer: 16 * 1024 * 1024});
        assert.ifError(result.error);
        return result;
    };
    const success = (command, args, cwd) => {
        const result = run(command, args, cwd);
        assert.equal(result.status, 0, result.stderr);
        return result.stdout;
    };
    const install = task => success("mise", ["run", "--skip-deps", task]);
    try {
        mkdirSync(source);
        mkdirSync(join(home, ".runtime"), {recursive: true, mode: 0o700});
        copyCheckout(root, source);
        cpSync(join(root, "dist/bin"), join(source, "dist/bin"), {recursive: true});
        mkdirSync(join(home, ".local/bin"), {recursive: true});
        const binary = join(home, ".local/bin/dots");
        writeFileSync(binary, "unmanaged executable\n");
        writeFileSync(join(home, ".local/.crates2.json"), '{"owner":"another installer"}');
        assert.notEqual(run("mise", ["run", "--skip-deps", "//src/dots:install"]).status, 0);
        assert.equal(readFileSync(binary, "utf8"), "unmanaged executable\n");
        rmSync(binary);
        install("//src/dots:install");
        install("//src/dots:install");
        const review = join(home, ".local/bin/herdr-review");
        writeFileSync(review, "unmanaged review executable\n");
        assert.notEqual(run("mise", ["run", "--skip-deps", "//src/herdr:install"]).status, 0);
        assert.equal(readFileSync(review, "utf8"), "unmanaged review executable\n");
        rmSync(review);
        install("//src/herdr:install");
        install("//src/herdr:install");
        assert.match(success(binary, ["--help"]), /github/);
        assert.ok(lstatSync(join(home, ".local/bin/herdr-review")).isSymbolicLink());
        JSON.parse(readFileSync(join(home, ".config/herdr/plugins.json"), "utf8"));
        assert.equal(readFileSync(join(home, ".local/.crates2.json"), "utf8"), '{"owner":"another installer"}');
        assert.ok(!existsSync(join(home, ".local/.crates.toml")));
        for (const file of ["home/herdr/.config/herdr/plugins.json", "home/skills/.local/share/dots"])
            assert.ok(!existsSync(join(source, file)), `runtime data leaked into source: ${file}`);
        for (const directory of [".config/herdr", ".local/share"])
            assert.ok(!lstatSync(join(home, directory)).isSymbolicLink(), `folded runtime directory: ${directory}`);
    } finally {
        rmSync(temporary, {recursive: true, force: true});
        assert.deepEqual(staging(), before, "installation changed real source staging");
    }
});
