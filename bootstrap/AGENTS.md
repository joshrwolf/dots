# Bootstrap development

Keep provisioning in native mise declarations. The source project acquires
committed repositories and registers modules; a fresh HOME pass applies them.
Do not load the repository development SDK to provision a machine.

Use [bootstrap acceptance](../src/bootstrap/README.md) for source/file checks.
Use disposable homes and explicit app destinations; a temporary HOME alone does
not isolate macOS application installation. Checks never activate the live setup.

## Unpublished candidates

In private staging, copy `source/` and copy `candidates.toml` to
`mise.local.toml`. Include only the selected bundles:

```sh
git -C "$public_checkout" bundle create "$stage/public.bundle" HEAD "$(git -C "$public_checkout" symbolic-ref HEAD)"
git -C "$private_checkout" bundle create "$stage/private.bundle" HEAD "$(git -C "$private_checkout" symbolic-ref HEAD)"
```

Omit the private bundle for public-only setup. Inspect the payload before
transfer. Bundles carry committed source, never ignored files or WIP.
Native copies retain them under `~/.local/share/dots/source-bundles` with a
private directory and mode 0600 files; repositories use those durable origins.
Recreate both the source templates and bundles from the reviewed candidate
before an explicit update. Do not change an acquired repository's origin
implicitly or reset dirty work.

Retain execution receipts, platform coverage, adoption decisions and rollback
notes in local tasks/evidence, not READMEs. A source/file test is not OS package,
SSH, provider-authorization or service-activation acceptance.
