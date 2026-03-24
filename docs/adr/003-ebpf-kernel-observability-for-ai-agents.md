# ADR-003: eBPF Kernel Observability for AI Agents

**Status:** Accepted (not yet implemented)
**Date:** 2026-03-24

## Context

RAUTA's diagnostics engine currently produces structured diagnoses from application-level data: circuit breaker state, rate limiter tokens, route health scores. When an AI agent asks "why is /api slow?", the best answer is:

```
Circuit breaker open. 5 failures in 30 seconds.
```

That's **what** happened. Not **why**. The agent hits a dead end. It can't reason further because the gateway doesn't see below HTTP.

Meanwhile, the Linux kernel knows exactly what's happening at the TCP level — RTT spikes, retransmits, congestion window collapse, OOM kills — but this data lives in kernel memory, invisible to the application and to agents.

## Decision

Add eBPF sensors that feed kernel-level TCP health data into RAUTA's diagnostics engine and MCP tools. The goal is not faster circuit breaking — it's giving AI agents kernel-level evidence they can reason about.

## The Value

An SRE's pager fires at 3am. `/api/checkout` is returning 503s.

**Without eBPF:** Open Grafana, see circuit breaker is open, SSH into nodes, check `ss -ti` for TCP stats, grep logs across 40 pods, correlate timestamps manually, find the OOM kill in a different dashboard. 45 minutes.

**With eBPF:** Ask the agent "why is checkout down?" Agent calls `rauta_diagnose`:

```
Backend 10.0.1.5 — TCP RTT 450ms (was 1ms), cwnd collapsed.
12 retransmits on last 5 connections.
OOM kill in same cgroup 3s before degradation started.
→ Fix: raise memory limit on checkout-service (currently 256Mi).
```

45 minutes → 30 seconds. The agent saw what the kernel saw.

## Architecture

```
Kernel (eBPF sockops)
  │ RTT, retransmits, cwnd per connection — zero copy, zero overhead
  ▼
BPF HashMap<ConnKey, TcpHealthMetrics>
  │ ring buffer events on threshold crossing
  ▼
Userspace processor (tokio task)
  │ mpsc::Sender<SensorEvent>
  ▼
DiagnosticsEngine
  │ RAUTA-TCP-001: TCP health degrading
  │ RAUTA-TCP-002: connection storm detected
  │ RAUTA-TCP-003: cwnd collapse (congestion)
  ▼
MCP Tools (rauta_diagnose)
  │ Agent gets kernel evidence in structured JSON
  ▼
FALSE Protocol → AHTI
  │ gateway.tcp.health.degraded occurrence
  │ Causality engine links TCP degradation to upstream events
  ▼
Agent reasons across the full stack
```

### Components

**`bpf-common/`** — `#[repr(C)]` shared types between kernel and userspace. `no_std`, zero deps.

```rust
#[repr(C)]
pub struct ConnKey {
    pub src_addr: [u8; 16],   // IPv4 or IPv6
    pub dst_addr: [u8; 16],
    pub src_port: u16,
    pub dst_port: u16,
    pub family: u8,           // AF_INET or AF_INET6
    pub _pad: [u8; 3],
}

#[repr(C)]
pub struct TcpHealthMetrics {
    pub srtt_us: u32,         // Smoothed RTT in microseconds
    pub retransmits: u32,     // Total retransmits
    pub snd_cwnd: u32,        // Send congestion window
    pub bytes_sent: u64,
    pub bytes_recv: u64,
    pub state: u8,            // TCP state
    pub _pad: [u8; 7],
}
```

**`bpf-probes/`** — Aya eBPF programs. NOT a workspace member (different target triple, nightly).

| Program | Hook | Data |
|---------|------|------|
| `rauta-sockops` | cgroup sockops | TCP health per connection: RTT, retransmits, cwnd |

Single program to start. kprobe connect latency and OOM tracepoints are future additions.

**`control/src/ebpf/`** — Userspace integration, all behind `#[cfg(target_os = "linux")]`.

| File | Purpose |
|------|---------|
| `loader.rs` | Load BPF programs. Graceful degradation: `Ok(Unavailable)` on macOS/missing CAP_BPF |
| `processor.rs` | Ring buffer consumer → `mpsc::Sender<SensorEvent>` |
| `tcp_health.rs` | Health score computation (0-100) from RTT, retransmits, cwnd |
| `enhanced_breaker.rs` | Proactively opens circuits when TCP health drops below threshold |

### Diagnostic Rules (new)

| Rule | Trigger | What the agent sees |
|------|---------|---------------------|
| RAUTA-TCP-001 | RTT > 10x baseline for a backend | "TCP RTT to 10.0.1.5 jumped from 1ms to 450ms" |
| RAUTA-TCP-002 | > 5 retransmits in 10s window | "12 retransmits on connections to 10.0.1.5" |
| RAUTA-TCP-003 | cwnd < 25% of peak | "Congestion window collapsed from 64KB to 2KB" |

Each rule produces evidence that the agent can chain into causal reasoning.

### Graceful Degradation

```rust
pub enum SensorAvailability {
    Available(BpfSensorManager),
    Unavailable { reason: String }, // macOS, no CAP_BPF, old kernel
}
```

When eBPF is unavailable:
- Proxy works identically (zero impact)
- Diagnostics engine runs with fewer evidence sources
- MCP tools return HTTP-level data only
- Agent still works, just with less depth

### Testing

- **macOS/CI:** Mock `SensorEvent` injection via mpsc channel. Test health scoring, enhanced breaker logic, diagnostic rules — all without a kernel.
- **Linux:** Load real BPF programs, generate TCP traffic, verify events flow through the full pipeline.

## Why No Other Gateway Does This

- **Envoy/NGINX/HAProxy:** No eBPF integration. Agents see "502 Bad Gateway" and nothing else.
- **Cilium:** Has eBPF but it's for network policy, not application-level diagnostics. No MCP interface.
- **Datadog/Pixie:** eBPF observability, but dashboards for humans. Agents can't query "why is this backend slow?" and get structured causal evidence.

RAUTA's vertical integration — kernel sensors → diagnostics engine → MCP tools → agent reasoning — is unique.

## What This Is NOT

- Not a replacement for the reactive circuit breaker (that still works as the safety net)
- Not a monitoring tool (we don't store time series — AHTI does that)
- Not required for RAUTA to function (graceful degradation is core)

## Implementation Order

1. `bpf-common/` types (works everywhere)
2. `control/src/ebpf/` userspace with stubs and mock injection
3. `tcp_health.rs` scoring + 3 diagnostic rules
4. `bpf-probes/` sockops program (Linux only)
5. `loader.rs` + `processor.rs` wiring
6. FALSE Protocol `gateway.tcp.health.degraded` occurrences

## Consequences

- RAUTA becomes the only gateway where AI agents have kernel-level visibility
- Incidents that took 45 minutes of manual correlation take 30 seconds
- The diagnostics engine's causal chains go from "what happened" to "why it happened"
- The FALSE Protocol occurrences let AHTI build cross-tool causal graphs that include kernel events
