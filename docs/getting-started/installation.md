# Installation

## Building from Source

Currently, Datalove is only available by building from source.

### Prerequisites

- Rust toolchain (1.85+ recommended)
- Git

### Clone and Build

```bash
git clone https://github.com/yourusername/datalove.git
cd datalove
cargo build --release
```

### Install

```bash
cargo install --path crates/datalove-cli
```

Or add the binary to your PATH:

```bash
export PATH="$PATH:$(pwd)/target/release"
```

### Verify Installation

```bash
datalove --help
```

You should see the Datalove CLI help message.

## Next Steps

Continue to [First Steps](first-steps.md) to write your first Datalove program.
