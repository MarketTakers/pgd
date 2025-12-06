```
▄▄▄▄      ▐▌
█   █     ▐▌
█▄▄▄▀  ▗▞▀▜▌
█   ▗▄▖▝▚▄▟▌
▀  ▐▌ ▐▌    
    ▝▀▜▌    
   ▐▙▄▞▘  
```  
---

> Inspired by the great [Gel](https://github.com/geldata/gel-cli) CLI. We will miss it.

Project-scoped PostgreSQL instance manager for local development.

## Overview

Tired of juggling PostgreSQL versions across projects? Wrestling with port conflicts? Spending half your morning helping new teammates get their local database running? Well, say no more!

`pgd` gives each of your projects its own containerized PostgreSQL instance with zero configuration headaches. 
Isolate, upgrade and nuke -- everything safely.

## Why Use pgd?

**Stop playing around database -- play with it.** Your legacy project needs Postgres 14, your new microservice wants 16, and that experimental side project is testing 19-beta. With `pgd`, they all run simultaneously without stepping on each other's toes.

**Onboard developers in seconds, not hours.** No more wiki pages with 47 steps to set up the local database. New teammate clones the repo, runs `pgd init`, and they're ready to code. The database config lives right there in version control where it belongs.

**Isolate your data like you isolate your code (or your life).** Each project gets its own database instance.

**Let the tool handle the boring stuff.** `pgd` manages ports, volumes and versions for you

## Requirements
- Docker daemon running locally
- Rust toolchain (for installation from source)

## Installation

Install via cargo:

```bash
cargo install pgd
```

## Quick Start

Navigate to your project directory and initialize a new PostgreSQL instance:

```bash
cd my-project
pgd init
```

This creates a `pgd.toml` configuration file with auto-generated credentials and latests postgres version available.
Note: upgrades are currently unsupported at the moment.
Downgrades wouldn't ever be supported, because postgres is not future-compatible.

## Commands

### Project Initialization

```bash
pgd init
```

Creates a `pgd.toml` file in the current directory with auto-populated configuration. If the file already exists, initializes the Docker container for the existing configuration.

### Instance Control

All instance commands follow the pattern `pgd instance <command>`:

```bash
# Check instance status and configuration drift
pgd instance status

# View PostgreSQL logs
pgd instance logs

# Follow logs in real-time
pgd instance logs --f

# Get connection details
pgd instance conn

# Get connection as DSN URL
pgd instance conn --format dsn

# Get human-readable connection details
pgd instance conn --format human
```

### Destructive Operations

```bash
# Remove the container
pgd instance destroy

# Wipe all database data
pgd instance wipe
```

These commands require confirmation to prevent accidental data loss.

### Global Options

```bash
# Enable verbose logging
pgd --verbose <command>

# Show version
pgd --version

# Show help
pgd --help
```

## How It Works

`pgd` manages Docker containers with PostgreSQL images. Each project's container is named deterministically based on the project directory name, ensuring no duplicates.

The tool tracks state separately for each instance to detect configuration drift, such as:
- Version mismatches between `pgd.toml` and the running container
- Port conflicts or changes
- Container state inconsistencies

When drift is detected, `pgd instance status` will show warnings and correct things.

## Project Structure

Your project tree after initialization:

```
my-project/
├── pgd.toml          # Database configuration
├── src/              # Your application code
└── ...
```

The `pgd.toml` file should be committed to version control so team members can reproduce the exact database setup.
