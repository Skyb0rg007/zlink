# Examples

This directory contains examples demonstrating the usage of the zlink library.

## resolved

A CLI tool that resolves hostnames to IP addresses using `systemd-resolved`'s Varlink service.

### Description

The `resolved` example demonstrates how to use the `#[proxy]` macro to generate a type-safe client
API, and how to use request pipelining to send multiple resolution requests at once. It connects to
`systemd-resolved` over its Unix domain socket and prints the resolved addresses for each hostname.

### Usage

```bash
# Resolve one or more hostnames
cargo run --example resolved -- example.com systemd.io
```

## varlink-inspect

A CLI tool for inspecting Varlink services via Unix domain sockets.

### Description

The `varlink-inspect` example demonstrates how to use the `varlink_service::Proxy` trait to
introspect Varlink services. It connects to a Unix domain socket, retrieves service information,
and optionally provides detailed interface descriptions with parsed IDL information.

### Basic Usage

```bash
# Show service information and available interfaces
cargo run --example varlink-inspect --features="introspection idl-parse" --
/run/systemd/machine/io.systemd.Machine

# Get detailed interface description with methods, types, and documentation
cargo run --example varlink-inspect --features="introspection idl-parse" --
/run/systemd/machine/io.systemd.Machine io.systemd.Machine
```

### Example Output

When inspecting the systemd machine interface, you'll see:

```
🔍 Service Information
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  Vendor:     The systemd Project
  Product:    systemd (systemd-machined)
  Version:    257.5 (257.5-6.fc42)
  URL:        https://systemd.io/

📋 Available Interfaces (4):
  1. io.systemd
  2. io.systemd.Machine
  3. io.systemd.MachineImage
  4. org.varlink.service

🔧 Interface: io.systemd.Machine
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

Methods (6):
  📞 Open
    📝 Allocates a pseudo TTY in the container in various modes
    ➡️ name: ?string
      📝 If non-null the name of a machine.
    ➡️ pid: ?ProcessId
      📝 If non-null the PID of a machine. Special value 0 means to take pid of the machine the caller is part of.
    ➡️ allowInteractiveAuthentication: ?bool
      📝 Controls whether interactive authentication (via polkit) shall be allowed. If unspecified defaults to false.
    ➡️ mode: MachineOpenMode
      📝 There are three possible values: 'tty', 'login', and 'shell'. Please see description for each of the modes.
    ➡️ user: ?string
      📝 See description of mode='shell'. Valid only when mode='shell'
    ➡️ path: ?string
      📝 See description of mode='shell'. Valid only when mode='shell'
    ➡️ args: ?[string]
      📝 See description of mode='shell'. Valid only when mode='shell'
    ➡️ environment: ?[string]
      📝 See description of mode='shell'. Valid only when mode='shell'
    ⬅️ ptyFileDescriptor: int
      📝 File descriptor of the allocated pseudo TTY
    ⬅️ ptyPath: string
      📝 Path to the allocated pseudo TTY
  ...

Custom Types (5):
  🏗️ AcquireMetadata (enum)
     📝 A enum field allowing to gracefully get metadata
     Variants:
       • no
       • yes
       • graceful

  🏗️ ProcessId (object)
     📝 An object for referencing UNIX processes
     Fields:
       • pid: int
         📝 Numeric UNIX PID value
       • pidfdId: ?int
         📝 64bit inode number of pidfd if known
  ...

Errors (8):
  ⚠️ NoSuchMachine
    📝 No matching machine currently running
  ⚠️ MachineExists
  ⚠️ NoPrivateNetworking
    📝 Machine does not use private networking
  ...

  📊 Summary:
    • 6 methods
    • 5 custom types
    • 8 error types
```
