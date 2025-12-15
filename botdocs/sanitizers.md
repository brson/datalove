# Rust Sanitizer Testing

Comprehensive testing infrastructure for running tests under all supported Rust sanitizers.

## Quick Start

```bash
# Run stable sanitizers (recommended for CI)
just test-sanitizers-stable

# Run all sanitizers (requires nightly)
just test-sanitizers-all

# Run individual sanitizer
just test-san-address
```

## Filtering Tests

All sanitizer commands accept optional arguments to filter which tests run:

```bash
# Run only specific package
just test-san-address -p datalove-datalit

# Run only library tests
just test-san-address --lib

# Run only tests matching a name pattern
just test-san-address my_test_name

# Combine filters
just test-san-address -p datalove-rt --lib test_foo

# Run with single thread (useful for debugging)
just test-san-address -- --test-threads=1
```

Arguments are passed directly to `cargo test`, so all cargo test filtering options work.

## Supported Sanitizers

### AddressSanitizer (address)
**Command:** `just test-san-address`
**Platform:** Linux, macOS, Windows
**Requires:** nightly

Detects memory errors:
- Use-after-free
- Heap/stack/global buffer overflows
- Use-after-return
- Use-after-scope
- Double-free/invalid-free

### LeakSanitizer (leak)
**Command:** `just test-san-leak`
**Platform:** Linux, macOS
**Requires:** nightly

Detects memory leaks at program termination.

### MemorySanitizer (memory)
**Command:** `just test-san-memory`
**Platform:** Linux only
**Requires:** nightly, `-Zbuild-std`

Detects reads of uninitialized memory. Requires building std from source.

**Prerequisites:**
```bash
rustup component add rust-src --toolchain nightly
```

### ThreadSanitizer (thread)
**Command:** `just test-san-thread`
**Platform:** Linux, macOS
**Requires:** nightly, `-Zbuild-std`

Detects data races and deadlocks in concurrent code.

**Prerequisites:**
```bash
rustup component add rust-src --toolchain nightly
```

### HWAddressSanitizer (hwaddress)
**Command:** `just test-san-hwaddress`
**Platform:** ARM64 only
**Requires:** nightly, `-Zbuild-std`

Hardware-assisted variant of AddressSanitizer with lower overhead.

### ControlFlowIntegrity (cfi)
**Command:** `just test-san-cfi`
**Platform:** Linux
**Requires:** nightly, `-Zbuild-std`, LTO

Detects control flow hijacking attacks.

## Prerequisites

Install nightly toolchain and rust-src:
```bash
rustup toolchain install nightly
rustup component add rust-src --toolchain nightly
```

## CI Integration

For CI, recommend running stable sanitizers:
```yaml
- name: Run sanitizer tests
  run: just test-sanitizers-stable
```

## Notes

- Sanitizers add significant overhead; tests will run slower.
- Some sanitizers cannot be combined (e.g., address + thread).
- MemorySanitizer may require `-Zbuild-std` for all dependencies.
- Some tests may need to be excluded if incompatible with sanitizers.

## Troubleshooting

If you encounter issues with dependencies not being instrumented, you may need to rebuild them:
```bash
cargo clean
just test-san-address
```

For memory/thread sanitizers, ensure rust-src is installed for your nightly toolchain.
