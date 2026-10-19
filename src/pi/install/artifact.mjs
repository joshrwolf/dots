import assert from "node:assert/strict";
import {createHash} from "node:crypto";
import {spawnSync} from "node:child_process";
import {existsSync, lstatSync, readdirSync, readFileSync, readlinkSync, writeFileSync} from "node:fs";
import {dirname, join, relative, resolve} from "node:path";
import {fileURLToPath} from "node:url";

export const root = fileURLToPath(new URL("../../../", import.meta.url));
export const build = join(root, "dist/pi-package");
export const runtime = join(root, "dist/pi-runtime");
const hash = (path, algorithm = "sha256", encoding = "hex") => createHash(algorithm).update(readFileSync(path)).digest(encoding);
export function run(command, args, cwd) {
    const result = spawnSync(command, args, {cwd, encoding: "utf8", timeout: 180000, maxBuffer: 32 * 1024 * 1024});
    if (result.error || result.status !== 0) throw new Error(`${command}: ${result.error?.message ?? result.stderr}\n${result.stdout}`);
    return result.stdout;
}
export function inventory(directory) {
    const entries = [];
    function walk(path) {
        const stat = lstatSync(path);
        if (stat.isSymbolicLink()) {
            const link = readlinkSync(path);
            const destination = relative(directory, resolve(dirname(path), link));
            assert.ok(destination !== ".." && !destination.startsWith("../"), "symlink escapes runtime");
            entries.push([relative(directory, path), "link:" + link]);
        } else if (stat.isDirectory()) {
            for (const name of readdirSync(path).sort()) walk(join(path, name));
        } else {
            assert.ok(stat.isFile(), "unsupported artifact entry");
            entries.push([relative(directory, path), hash(path)]);
        }
    }
    walk(directory);
    return entries;
}
export function artifact() {
    const report = JSON.parse(readFileSync(join(build, "artifact.json"), "utf8"));
    assert.ok(Array.isArray(report) && report.length === 1, "expected one npm package");
    const [info] = report;
    assert.match(info.filename, /^[a-z0-9._-]+\.tgz$/i);
    assert.deepEqual(readdirSync(build).sort(), ["artifact.json", info.filename].sort(), "unexpected package output");
    assert.equal("sha512-" + hash(join(build, info.filename), "sha512", "base64"), info.integrity, "artifact integrity mismatch");
    assert.ok(info.files.some(file => file.path === "npm-shrinkwrap.json"), "runtime lock missing");
    assert.ok(info.files.every(file => /^(extensions|src|themes)\//.test(file.path) || ["package.json", "npm-shrinkwrap.json", "README.md", "LICENSE"].includes(file.path)), "unexpected package contents");
    return info;
}
export function verifyRelease(directory) {
    const ready = JSON.parse(readFileSync(join(directory, ".dots-ready.json"), "utf8"));
    const files = inventory(directory).filter(([name]) => name !== ".dots-ready.json");
    assert.deepEqual(files, ready.files, "runtime changed after preparation");
    return ready;
}
export function verifyRuntime(directory = runtime) {
    const ready = verifyRelease(directory);
    assert.equal(ready.integrity, artifact().integrity, "runtime is stale; prepare it again");
    return ready;
}
function validateArchive() {
    const info = artifact();
    const names = run("tar", ["-tzf", join(build, info.filename)], root).trim().split("\n");
    assert.ok(names.every(name => name.startsWith("package/") && !name.split("/").includes("..")), "unsafe archive paths");
}
function sealRuntime() {
    for (const name of ["@earendil-works/pi-coding-agent", "@earendil-works/pi-tui", "typebox"])
        assert.ok(!existsSync(join(runtime, "node_modules", name)), "host peer physically installed: " + name);
    const manifest = JSON.parse(readFileSync(join(runtime, "package.json"), "utf8"));
    for (const paths of Object.values(manifest.pi)) for (const path of paths)
        assert.ok(existsSync(join(runtime, path)), "missing resource " + path);
    assert.ok(existsSync(join(runtime, "extensions/jobs/supervisor.ts")), "missing subprocess entrypoint");
    const selected = artifact();
    const files = inventory(runtime).filter(([name]) => name !== ".dots-ready.json");
    // Native npm reproduces the package identity from the extracted bytes;
    // a newer artifact report alone cannot certify an older runtime.
    const packed = JSON.parse(run("npm", ["pack", runtime, "--dry-run", "--offline", "--ignore-scripts", "--json"], root));
    assert.equal(packed.length, 1, "expected one runtime package");
    assert.equal(packed[0].integrity, selected.integrity, "runtime does not match packaged artifact");
    writeFileSync(join(runtime, ".dots-ready.json"), JSON.stringify({integrity: selected.integrity, files}, null, 2) + "\n");
    verifyRuntime();
}
if (process.argv[1] === fileURLToPath(import.meta.url)) {
    switch (process.argv[2]) {
        case "validate-archive": validateArchive(); break;
        case "seal-runtime": sealRuntime(); break;
        case "verify": verifyRuntime(); break;
        default: throw new Error("Expected validate-archive, seal-runtime, or verify");
    }
}
