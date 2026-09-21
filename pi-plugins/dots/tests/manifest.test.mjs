import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const root = fileURLToPath(new URL("../", import.meta.url));
const manifest = JSON.parse(readFileSync(new URL("../package.json", import.meta.url), "utf8"));

test("personal package declares explicit, existing local resources", () => {
  assert.equal(manifest.private, true);
  assert.deepEqual(Object.keys(manifest.pi).sort(), ["extensions", "prompts", "skills", "themes"]);
  for (const resources of Object.values(manifest.pi)) {
    assert.ok(Array.isArray(resources));
    assert.equal(new Set(resources).size, resources.length);
    for (const resource of resources) {
      assert.equal(typeof resource, "string");
      const relative = path.relative(root, path.resolve(root, resource));
      assert.ok(relative && !relative.startsWith("..") && !path.isAbsolute(relative));
      assert.ok(existsSync(path.resolve(root, resource)), `Missing resource: ${resource}`);
    }
  }
});
