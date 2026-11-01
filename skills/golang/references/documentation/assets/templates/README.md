# {project-name}

<!-- Replace {owner} and {repo} throughout this file. Keep license links only
     when they point to the project's actual license file; adapt other badges and
     relative links to the project layout. -->

[![Go Version](https://img.shields.io/github/go-mod/go-version/{owner}/{repo})](https://go.dev/) [![License](https://img.shields.io/github/license/{owner}/{repo})](./LICENSE) [![Build Status](https://img.shields.io/github/actions/workflow/status/{owner}/{repo}/test.yml?branch=main)](https://github.com/{owner}/{repo}/actions) [![Coverage](https://img.shields.io/codecov/c/github/{owner}/{repo})](https://codecov.io/gh/{owner}/{repo}) [![Go Report Card](https://goreportcard.com/badge/github.com/{owner}/{repo})](https://goreportcard.com/report/github.com/{owner}/{repo}) [![Go Reference](https://pkg.go.dev/badge/github.com/{owner}/{repo}.svg)](https://pkg.go.dev/github.com/{owner}/{repo})

<!-- Additional badges (pick what's relevant):
[![Release](https://img.shields.io/github/v/release/{owner}/{repo})](https://github.com/{owner}/{repo}/releases)
[![Downloads](https://img.shields.io/github/downloads/{owner}/{repo}/total)](https://github.com/{owner}/{repo}/releases)
[![Docker Pulls](https://img.shields.io/docker/pulls/{owner}/{repo})](https://hub.docker.com/r/{owner}/{repo})
-->

<!-- 1-2 sentences: what does this project do and who is it for? -->

<!-- Show the project in action: code snippet, GIF, screenshot, or video.
     For libraries: show a minimal working code example.
     For CLIs/tools: a GIF or screenshot is often more effective. -->

```go
// Minimal working example showing the most common use case
```

## 🚀 Getting Started

<!-- For libraries: -->

```bash
go get github.com/{owner}/{repo}
```

```go
package main

import "github.com/{owner}/{repo}"

func main() {
    // Minimal working example
}
```

<!-- For applications, uncomment and use this instead:

### Pre-built binaries

Download from [GitHub Releases](https://github.com/{owner}/{repo}/releases/latest).

| Platform | Architecture | Download                                                                                        |
| -------- | ------------ | ----------------------------------------------------------------------------------------------- |
| Linux    | amd64        | [Download](https://github.com/{owner}/{repo}/releases/latest/download/{repo}-linux-amd64)       |
| Linux    | arm64        | [Download](https://github.com/{owner}/{repo}/releases/latest/download/{repo}-linux-arm64)       |
| macOS    | amd64        | [Download](https://github.com/{owner}/{repo}/releases/latest/download/{repo}-darwin-amd64)      |
| macOS    | arm64        | [Download](https://github.com/{owner}/{repo}/releases/latest/download/{repo}-darwin-arm64)      |
| Windows  | amd64        | [Download](https://github.com/{owner}/{repo}/releases/latest/download/{repo}-windows-amd64.exe) |

### From source

```bash
go install github.com/{owner}/{repo}@latest
```

### Docker

```bash
docker pull {registry}/{owner}/{repo}:latest
docker run --rm {registry}/{owner}/{repo}:latest --help
```

### Homebrew (macOS)

```bash
brew install {owner}/{tap}/{formula}
```

### APT (debian/ubuntu)

```bash
apt install {package}
```

-->

## ✨ Features

<!-- Describe supported features; keep detail proportional to the project. -->

### Feature Area 1

<!-- Description with code examples -->

### Feature Area 2

<!-- Description with code examples -->

## 🤝 Contributing

Please read the [contributing guide](CONTRIBUTING.md) before submitting a PR.

<!-- Or if the contributing guide is very short:

```bash
# Build
go build -o myapp ./cmd/myapp

# Run unit tests
go test -race ./...

# Run integration tests
go test -race -tags=integration -timeout=300s ./...

# Run linter
golangci-lint run ./...
```
-->

## 📄 License

<!-- State the project's actual license; do not assume MIT. -->
See [LICENSE](LICENSE).
