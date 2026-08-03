# Roadmap

Current state: **alpha** — Docker-only, local engine, GUI + CLI on the
same core.

## Planned

### Compose projects
Docker Compose project list/up/down/logs. Design will be done
separately; no shell-command splicing. The GUI already shows an honest
"planned" Compose page.

### Container terminal
A real PTY-based terminal (planned); no fake terminal.

### Container files
File browser/copy in/out (planned); no fake file manager.

### Image operations
Pull, build, tag, push, prune — with real progress from Docker, no
mock progress bars.

### Registry login
`docker login`-style credential handling, careful with secrets.

### Docker contexts
List/switch between `docker context` configurations.

### Remote engines
Connect over `tcp://`/`ssh://` hosts (connection plumbing already
exists in `DockerClient`).

## Future consideration

### Incus
Incus will be added as a **separate** crate: `crates/incus-core/`,
alongside docker-core:

```
gui
├── docker-core
└── incus-core
```

A shared `WorkloadBackend`-style abstraction will only be extracted
after real duplication between Docker and Incus code exists — never
before. The GUI keeps Docker-native terminology (Container, Image,
Network, Volume) and will not be renamed to generic terms.

### Podman
Not currently planned; revisit if demand appears.

## Explicitly out of scope for now

- Multiple backend plugin systems
- Universal resource models
- macOS/Windows support
