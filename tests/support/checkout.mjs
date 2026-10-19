import {execFileSync} from "node:child_process";
import {copyFileSync, lstatSync, mkdirSync, readlinkSync, symlinkSync} from "node:fs";
import {dirname, join} from "node:path";

// Copy the current authored tree, including uncommitted changes, without
// copying build outputs, credentials, Git metadata, or ignored local state.
export function copyCheckout(source, destination) {
    mkdirSync(destination, {recursive: true});
    const files = execFileSync("git", ["ls-files", "--cached", "--others", "--exclude-standard", "-z"],
        {cwd: source, encoding: "utf8", timeout: 10_000, maxBuffer: 16 * 1024 * 1024});
    for (const file of new Set(files.split("\0").filter(Boolean).map(path => path.replace(/\/+$/, "")))) {
        // Git still lists deleted index entries beneath a directory replaced
        // by a symlink. Do not follow that link into an ignored/external tree.
        let parent = source;
        const redirected = file.split("/").slice(0, -1).some(component => {
            parent = join(parent, component);
            return !lstatSync(parent, {throwIfNoEntry: false})?.isDirectory();
        });
        if (redirected) continue;
        const original = join(source, file), target = join(destination, file);
        const entry = lstatSync(original, {throwIfNoEntry: false});
        if (!entry || (!entry.isFile() && !entry.isSymbolicLink())) continue;
        mkdirSync(dirname(target), {recursive: true});
        if (entry.isSymbolicLink()) symlinkSync(readlinkSync(original), target);
        else copyFileSync(original, target);
    }
}
