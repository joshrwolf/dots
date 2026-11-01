# Go documentation

Document the contract a caller needs: purpose, accepted inputs, errors, ownership,
concurrency safety and limitations. Do not restate signatures or invent rationale.
Follow the project's existing documentation structure and release conventions.

- Start exported declaration comments with the declared name. Package comments
  begin with `Package name`; use `# Heading` for sections and `[Identifier]` for
  symbol links. Put a substantial package overview in `doc.go` when it needs its
  own home, not simply because a package has several files.
- Explain observable error and nil behavior without promising incidental
  implementation details. Use `Deprecated:` only when the API has a real
  deprecation policy.
- Use executable `ExampleXxx` functions for usage that benefits from a checked
  example. An `Output:` or `Unordered output:` comment makes it run under
  `go test`; without either, it only compiles.
- Inspect resolved package comments with `go doc` inside the module. Do not
  publish private code to a playground or public documentation service.
- Keep CLI help and API schemas aligned with actual behavior; use the project's
  existing validation or generation workflow.

Read further only for the relevant task:

- [Code comments](documentation/code-comments.md): comment syntax, contracts,
  package overviews and symbol links.
- [Libraries](documentation/library.md): examples and package documentation.
- [Applications](documentation/application.md): CLI help, configuration and API
  schemas. `golang-cli`, `golang-grpc` and `golang-graphql` own implementation.
- [Project documentation](documentation/project-docs.md): release notes and
  installation documentation, with optional templates. `golang-continuous-integration`
  owns release automation.
