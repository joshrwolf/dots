import assert from "node:assert/strict";
import {test} from "node:test";
import {upstreamPlan} from "../install/index.mjs";

test("native Pi actions preserve registrations and resource filters", () => {
    const entries = [{source: "npm:example@1.2.3", extensions: ["!disabled.ts"]}];
    const original = structuredClone(entries);
    assert.deepEqual(upstreamPlan(["npm:example@1.2.3", "npm:@example/other@2.0.0"], entries, "/tmp/agent"),
        [["update", "--no-approve", "npm:example@1.2.3"], ["install", "--no-approve", "npm:@example/other@2.0.0"]]);
    assert.deepEqual(entries, original);
});
test("floating and duplicate pins fail before native installation", () => {
    for (const pins of [["npm:example@latest"], ["npm:example@^1.2.3"], ["npm:example@1.2.3", "npm:example@2.0.0"]])
        assert.throws(() => upstreamPlan(pins, [], "/tmp/agent"));
});
test("conflicting versions and duplicate registrations are rejected", () => {
    for (const entries of [["npm:example@1.0.0"], ["npm:example@1.2.3", "npm:example@1.2.3"], ["git:https://example.com/example.git"]])
        assert.throws(() => upstreamPlan(["npm:example@1.2.3"], entries, "/tmp/agent"));
});
test("unrelated packages are left to their owner", () => {
    assert.deepEqual(upstreamPlan(["npm:example@1.2.3"], ["npm:other@9.0.0"], "/tmp/agent"),
        [["install", "--no-approve", "npm:example@1.2.3"]]);
});
test("malformed inputs are not silently accepted", () => {
    assert.throws(() => upstreamPlan({}, [], "/tmp/agent"));
    assert.throws(() => upstreamPlan([], [null], "/tmp/agent"));
});
