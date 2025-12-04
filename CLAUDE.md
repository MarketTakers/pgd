# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

`pgd` is a CLI tool for managing project-scoped PostgreSQL instances running in Docker containers. Each project gets its own isolated Postgres instance, managed through a `pgd.toml` configuration file.

## Core Architecture

### Project-Oriented Design

- Each project has a `pgd.toml` file at its root that defines the Postgres configuration
- The project name is derived from the directory containing `pgd.toml`
- Each project gets its own Docker container
- State is tracked separately per instance to detect configuration drift

### Configuration Management

The `pgd.toml` file stores:
- `postgres_version`: PostgreSQL version to use
- `database_name`: Name of the database
- `user_name`: Database user
- `password`: Database password
- `port`: Host port to bind (auto-selected from available ports)

Values are auto-populated during `pgd init` with sensible defaults or random values where appropriate.

### State Tracking

The tool maintains separate state for each instance to detect configuration drift, such as:
- Container's actual Postgres version vs. config file version
- Running container state vs. expected state
- Port conflicts or changes

## Key Dependencies

- **clap** (with derive feature): CLI argument parsing and command structure
- **toml**: Parsing and serializing `pgd.toml` configuration files
- **bollard**: Docker daemon interaction for container lifecycle management
- **tokio** (with full feature set): Async runtime for Docker operations
- **tracing** + **tracing-subscriber**: Structured logging throughout the application
- **serde**: Serialization/deserialization for config and state
- **miette** (with fancy feature): Enhanced error reporting
- **prodash** (to be added): Terminal progress bars for long-running operations

## Development Commands

### Building
```bash
cargo build
cargo build --release
```

### Running
```bash
cargo run -- <command>
# Example: cargo run -- init
```

### Testing
```bash
cargo test
cargo test <test_name>  # Run specific test
```

### Linting
```bash
cargo clippy
cargo clippy -- -W clippy::all
```

### Formatting
```bash
cargo fmt
cargo fmt -- --check  # Check without modifying
```

## Command Structure

The CLI follows this pattern:
```
pgd <command> [options]
```

Key commands to implement:
- `pgd init`: Create pgd.toml with auto-populated configuration
- `pgd start`: Start the Postgres container for current project
- `pgd stop`: Stop the running container
- `pgd status`: Show instance status and detect drift
- `pgd destroy`: Remove container and clean up

## Implementation Notes

### Port Selection

Port assignment should scan for available ports on the system rather than using fixed defaults to avoid conflicts.

### Docker Container Naming

Container names should be deterministic based on project name to allow easy identification and prevent duplicates.

### Error Handling

Use `miette` for user-facing errors with context. Docker operations and file I/O are the primary error sources.

### Async Operations

Docker operations via `bollard` are async. Use `tokio` runtime with `#[tokio::main]` on the main function.
