import assert from "node:assert/strict";
import {existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync} from "node:fs";
import {tmpdir} from "node:os";
import {join} from "node:path";
import {fileURLToPath, pathToFileURL} from "node:url";
import {test} from "node:test";
import {copyCheckout} from "../../../../tests/support/checkout.mjs";

const repository = fileURLToPath(new URL("../../../../", import.meta.url));

test("native npm artifacts are repeatable, restored from cache, and verified before activation", async t => {
    const scratch = mkdtempSync(join(tmpdir(), "dots-artifacts-"));
    t.after(() => rmSync(scratch, {recursive: true, force: true}));
    const fixture = join(scratch, "source");
    copyCheckout(repository, fixture);
    const logs = join(scratch, "npm-logs");
    mkdirSync(logs);
    const settings = {MISE_TRUSTED_CONFIG_PATHS: fixture, NPM_CONFIG_LOGS_DIR: logs, NPM_CONFIG_LOGS_MAX: "100"};
    const previous = Object.fromEntries(Object.keys(settings).map(key => [key, process.env[key]]));
    Object.assign(process.env, settings);
    t.after(() => {
        for (const [key, value] of Object.entries(previous)) {
            if (value === undefined) delete process.env[key];
            else process.env[key] = value;
        }
    });
    const {artifact, build, root, run, runtime, verifyRuntime} = await import(pathToFileURL(join(fixture, "src/pi/install/artifact.mjs")).href);
    const packageTask = () => run("mise", ["run", "//src/pi:package"], root);
    const forcePackage = () => run("mise", ["run", "--force", "//src/pi:package"], root);
    forcePackage();
    const initial = artifact().integrity;
    const firstLogs = readdirSync(logs);
    assert.ok(firstLogs.length > 0, "native npm must emit diagnostic logs for the cache observation");
    forcePackage();
    assert.equal(artifact().integrity, initial, "forced packaging must be repeatable");
    const executions = readdirSync(logs).sort();
    assert.ok(executions.length > firstLogs.length, "forced packaging must actually invoke npm");
    rmSync(build, {recursive: true});
    packageTask();
    assert.ok(existsSync(build));
    assert.equal(artifact().integrity, initial);
    assert.deepEqual(readdirSync(logs).sort(), executions, "cache restoration must not invoke npm");

    writeFileSync(join(build, artifact().filename), "corrupt");
    assert.throws(artifact, /integrity mismatch/);
    packageTask();
    assert.equal(artifact().integrity, initial, "mise must repair corrupted output");

    const changed = join(root, "src/pi/extensions/context/index.ts");
    const original = readFileSync(changed);
    writeFileSync(changed, Buffer.concat([original, Buffer.from("\n// fixture change\n")]));
    packageTask();
    assert.notEqual(artifact().integrity, initial, "resource edits must invalidate the output");
    writeFileSync(changed, original);
    packageTask();
    assert.equal(artifact().integrity, initial);

    run("mise", ["run", "//src/pi:prepare"], root);
    assert.ok(!existsSync(join(runtime, "dev")), "development workbench must not ship");
    for (const path of ["extensions/context/index.ts", "node_modules/diff/package.json"]) {
        const file = join(runtime, path), contents = readFileSync(file);
        writeFileSync(file, "corrupt");
        assert.throws(() => verifyRuntime(), /runtime changed/);
        writeFileSync(file, contents);
        verifyRuntime();
    }
    const readyPath = join(runtime, ".dots-ready.json");
    const ready = JSON.parse(readFileSync(readyPath, "utf8"));
    ready.integrity = "stale";
    writeFileSync(readyPath, JSON.stringify(ready));
    assert.throws(() => verifyRuntime(), /runtime is stale/);

    // Interleave extraction of A with publication of B before A is sealed.
    writeFileSync(changed, Buffer.concat([original, Buffer.from("\n// newer artifact\n")]));
    packageTask();
    assert.notEqual(artifact().integrity, initial);
    assert.throws(() => run("node", ["src/pi/install/artifact.mjs", "seal-runtime"], root),
        /runtime does not match packaged artifact/);
    assert.throws(() => verifyRuntime(), /runtime is stale/);
});
