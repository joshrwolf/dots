# Bootstrap acceptance

Test native mise source acquisition, module composition and file ownership in
disposable homes and committed Git repositories:

```sh
cargo nextest run --locked -p dots-bootstrap-tests --profile acceptance
```

Set `DOTS_TEST_MISE` to select a different mise executable, or
`DOTS_TEST_PRIVATE_BOOTSTRAP` to include a private module's `bootstrap/`
directory. Private skill fixtures use neutral data.

These checks do not install system packages or activate live services.
See [build and activation](../../docs/build.md) for repository-wide checks and
[machine setup](../../bootstrap/README.md) for installation.
