import assert from "node:assert/strict";
import {accessSync, constants, existsSync, lstatSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync} from "node:fs";
import {homedir} from "node:os";
import {join} from "node:path";
import {spawnSync} from "node:child_process";
import {fileURLToPath} from "node:url";
import {test} from "node:test";
import {copyCheckout} from "../support/checkout.mjs";

const root = fileURLToPath(new URL("../../", import.meta.url));

// A copied checkout and a disposable HOME keep plugin staging, links, and
// runtime state away from the real user. CARGO_INSTALL_ROOT redirects Cargo's
// default install root while the real CARGO_HOME keeps the toolchain and
// registry cache, and the shared target directory reuses compilation.
test("installation tasks put working commands on PATH and link plugins in place", () => {
    const temporary = mkdtempSync("/tmp/dci-"); // Herdr's Unix socket has a short path limit.
    const source = join(temporary, "source"), home = join(temporary, "home");
    const bin = join(home, ".cargo/bin");
    const env = {...Object.fromEntries(Object.entries(process.env).filter(([key]) => !key.startsWith("HERDR_"))),
        HOME: home, XDG_CONFIG_HOME: join(home, ".config"), XDG_DATA_HOME: join(home, ".local/share"),
        XDG_STATE_HOME: join(home, ".local/state"), XDG_CACHE_HOME: join(home, ".cache"), XDG_RUNTIME_DIR: join(home, ".runtime"),
        MISE_TRUSTED_CONFIG_PATHS: source,
        MISE_DATA_DIR: process.env.MISE_DATA_DIR ?? join(homedir(), ".local/share/mise"),
        MISE_CACHE_DIR: process.env.MISE_CACHE_DIR ?? join(homedir(), ".cache/mise"),
        CARGO_HOME: process.env.CARGO_HOME ?? join(homedir(), ".cargo"),
        CARGO_INSTALL_ROOT: join(home, ".cargo"),
        RUSTUP_HOME: process.env.RUSTUP_HOME ?? join(homedir(), ".rustup"),
        CARGO_TARGET_DIR: process.env.CARGO_TARGET_DIR ?? join(root, "target"),
    };
    const success = (command, args, extra = {}) => {
        const result = spawnSync(command, args, {cwd: source, env: {...env, ...extra}, encoding: "utf8", timeout: 180_000, maxBuffer: 16 * 1024 * 1024});
        assert.ifError(result.error);
        assert.equal(result.status, 0, result.stderr);
        return result.stdout;
    };
    try {
        mkdirSync(source);
        mkdirSync(join(home, ".runtime"), {recursive: true, mode: 0o700});
        copyCheckout(root, source);
        success("mise", ["run", "//src/dots:install"]);
        // Plugins build in the checkout and link in place; relinking is harmless.
        success("mise", ["run", "//src/herdr:install"]);
        success("mise", ["run", "//src/herdr:install"]);
        const path = {PATH: `${bin}:${process.env.PATH}`};
        for (const command of ["dots", "herdr-issue", "herdr-review"])
            assert.equal(success("sh", ["-c", `command -v ${command}`], path).trim(), join(bin, command));
        assert.match(success("dots", ["--help"], path), /github/);
        success("herdr-issue", ["--help"], path);
        // herdr-review has no help mode; its own rejection proves it runs.
        const review = spawnSync("herdr-review", ["--help"], {env: {...env, ...path}, encoding: "utf8", timeout: 10_000});
        assert.ifError(review.error);
        assert.match(review.stderr, /unsupported review process mode/);
        const plugins = JSON.parse(readFileSync(join(home, ".config/herdr/plugins.json"), "utf8"));
        assert.deepEqual(plugins.map(plugin => plugin.plugin_id).sort(), ["herdr-find", "herdr-github", "herdr-nav", "herdr-review"]);
        for (const plugin of plugins) {
            const name = plugin.plugin_id.replace(/^herdr-/, "");
            assert.equal(plugin.source.kind, "local");
            assert.equal(realpathSync(plugin.plugin_root), realpathSync(join(source, "src/herdr", name)));
            accessSync(join(plugin.plugin_root, "bin", plugin.plugin_id), constants.X_OK);
        }
        for (const file of ["home/herdr/.config/herdr/plugins.json", "home/herdr/.config/herdr/plugins", "home/skills/.local/share/dots"])
            assert.ok(!existsSync(join(source, file)), `runtime data leaked into source: ${file}`);
        for (const directory of [".config/herdr", ".config/herdr/plugins", ".local/share"])
            assert.ok(!lstatSync(join(home, directory), {throwIfNoEntry: false})?.isSymbolicLink(), `folded runtime directory: ${directory}`);
    } finally {
        rmSync(temporary, {recursive: true, force: true});
    }
});
