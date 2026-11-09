# AIOS — Artificial Intelligence Operating System

> **v0.1 Alpha** — an AI-native computer platform, built from scratch in Rust.

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](#license)
[![Rust](https://img.shields.io/badge/rust-nightly-orange.svg)](https://www.rust-lang.org/)
[![Status](https://img.shields.io/badge/status-alpha%20scaffold-059669.svg)]()
[![Target](https://img.shields.io/badge/target-x86__64%20UEFI-0891b2.svg)]()

AIOS is **not** a traditional OS with an AI layer bolted on top. It is an
AI-native platform where:

| Classic OS | AIOS |
|---|---|
| Applications | **Agents** |
| Files | **Knowledge Objects** (vector-embedded) |
| Drivers | **Generated Capabilities** (AI-authored, sandboxed) |
| The OS itself | **Cognitive Infrastructure** |

The AI runs **locally** (GGUF, quantized). The kernel is an **immutable
security base**: everything the AI generates — drivers, agents, tools — runs
in sandboxes above it and can never mutate the core.

## Boot flow

```
UEFI Firmware -> Bootloader (uefi-rs + NASM handoff)
              -> AIOS Kernel _start (RDI = &BootInfo)
              -> memory / interrupts / sched bring-up
              -> HAL -> Network Stack -> AI Engine
              -> Root Agent (federation) -> AI GUI (framebuffer)
```

## Repository layout

```
aios/
+-- crates/
|   +-- kernel/             # hybrid microkernel (no_std): boot, serial,
|   |                       #   logger, panic, BootInfo, memory, interrupts, sched
|   +-- bootloader/         # UEFI app (uefi-rs) + NASM handoff trampoline
|   +-- hal/                # device traits, device tree, capability bus
|   +-- ai-core/            # local GGUF runner, BPE tokenizer, context manager
|   +-- cognitive-runtime/  # planner, reasoner, decision engine
|   +-- agents/             # federation: root + network/hardware/coding agents
|   +-- knowledge/          # knowledge objects, vector index, memory tiers
|   +-- netstack/           # native TCP/IP, DNS, HTTP client (+ TLS stub)
|   +-- drivers/            # AI driver generation: PCI detect -> generate -> sandbox
|   +-- gui/                # framebuffer desktop, agent-centric windows
|   +-- libs/aios-sync/     # shared synchronization primitives
|   +-- libs/aios-log/      # shared logging vocabulary
+-- targets/x86_64-aios.json  # custom bare-metal LLVM target
+-- docs/ARCHITECTURE.md      # generated architecture map
+-- docs/LLD.md               # generated low-level design record
+-- Makefile                  # kernel / bootloader / boot-image / run
+-- rust-toolchain.toml       # pinned nightly
```

## Status (v0.1 alpha)

| Phase | Scope | State |
|---|---|---|
| 0 | Workspace, CI, target spec | shipped |
| 1 | UEFI boot + handoff, serial logging, panic handler | shipped |
| 2 | memory, interrupts, scheduler, HAL | in progress |
| 3 | AI engine (GGUF), cognitive runtime | in progress |
| 4 | Agent federation, knowledge system | in progress |
| 5 | Network stack (TCP/IP, DNS, HTTP) | in progress |
| 6 | AI driver generation + sandbox | in progress |
| 7 | AI GUI | in progress |

Phases 2-7 are **engineering scaffolds**: real interfaces and data models
under active development, not yet bootable end-to-end. The kernel crate
(Phase 0-1) is the most complete and is the reference for the house style:
safe Rust first, every `unsafe` block documented with a `SAFETY:` comment,
`panic = "abort"`, no unwinding.

## Quick start

Prerequisites: Rust nightly (rustup picks it up from `rust-toolchain.toml`),
`nasm`, `mtools`, `qemu-system-x86_64`, and OVMF firmware at `tools/OVMF.fd`.

```sh
# Build the kernel for the custom bare-metal target
make kernel

# Build the UEFI bootloader + FAT boot image (needs nasm + mtools)
make boot-image

# Boot under QEMU with the serial console attached
make run
```

On a successful boot the serial console prints:

```
[INFO]  AIOS v0.1-alpha - Cognitive Kernel
[INFO] [boot] BootInfo v1 accepted
[INFO] [boot] AIOS BOOT SUCCESS
```

## Security model — Immutable Core

1. The kernel owns memory, scheduling, IPC and capability-based security.
2. AI-generated drivers are compiled and loaded **only** through the
   sandbox (crates/drivers/src/sandbox.rs) with least-privilege
   capabilities — exactly the BARs the device needs.
3. Generated code implements HAL traits; it can never link against the
   kernel security base.
4. The decision engine drops any action that would mutate the kernel
   (crates/cognitive-runtime/src/decision.rs).

## Contributing

Issues AIOS-001 through AIOS-027 define the backlog (see docs/LLD.md).
PRs welcome — small, documented, SAFETY-commented.

## License

Dual-licensed under your choice of [MIT](LICENSE-MIT) or
[Apache-2.0](LICENSE-APACHE).

---

Author: **Jad Jbara** — jad@aios.dev
