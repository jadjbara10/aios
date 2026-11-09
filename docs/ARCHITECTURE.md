# AIOS Architecture

> Generated from the AIOS content payload. Do not hand-edit; regenerate with `bun run tools/extract-repo.ts`.

## Boot flow

1. **UEFI Firmware** — handoff
2. **Bootloader** — long mode
3. **AIOS Kernel** — _start
4. **HAL** — devices
5. **Network** — TCP/IP+TLS
6. **AI Engine** — GGUF local
7. **Root Agent** — federation
8. **AI GUI** — framebuffer

## Subsystem map

| Subsystem | Phase | Tagline | Code files | Issues |
|---|---|---|---|---|
| Memory Subsystem | Phase 2 | Frame allocator, kernel heap, paging activation | 4 | AIOS-006, AIOS-007 |
| Interrupt Subsystem | Phase 2 | GDT, TSS, IDT, exception handlers, 8259 PIC | 4 | AIOS-008, AIOS-009 |
| Scheduler & IPC | Phase 2 | Preemptive tasks, context switching, IPC channels | 4 | AIOS-010, AIOS-011 |
| Hardware Abstraction Layer | Phase 2 | Unified device traits, device tree, capability bus | 4 | AIOS-012, AIOS-013 |
| AI Engine | Phase 3 | Local GGUF runner, tokenizer, context manager | 4 | AIOS-014, AIOS-015 |
| Cognitive Runtime | Phase 3 | Planner, Reasoner, Decision Engine | 4 | AIOS-016, AIOS-017 |
| Multi-Agent Federation | Phase 4 | Root + specialist agents, Agent Message Bus | 4 | AIOS-018, AIOS-019 |
| Knowledge System | Phase 4 | Vector memory, Knowledge Objects, short/long term | 4 | AIOS-020, AIOS-021 |
| Network Stack | Phase 5 | Native TCP/IP, DNS, HTTP/TLS | 4 | AIOS-022, AIOS-023 |
| Driver Generation System | Phase 6 | AI detects hardware, generates, compiles, sandboxes drivers | 4 | AIOS-024, AIOS-025 |
| AI GUI | Phase 7 | Framebuffer desktop, agent-centric windows | 4 | AIOS-026, AIOS-027 |

## Core issues (Phase 0-1)

- **AIOS-001** — Create aios-os organization & monorepo scaffolding (done)
- **AIOS-002** — Setup Rust CI for no_std + custom target builds (done)
- **AIOS-003** — UEFI boot path: bootloader -> kernel handoff (done)
- **AIOS-004** — Kernel logging subsystem: serial + log facade (done)
- **AIOS-005** — Memory: frame allocator + kernel heap (bump + buddy) (open)

## Security model

- Immutable Core: the kernel security base is not mutable by AI-generated code.
- Capability-based access: every device/action requires a Capability<T>.
- Sandboxed generation: AI drivers compile + load only through the sandbox.

