# OS scheduling policies: priorities, time budgets, and fairness

Operating systems have developed surprisingly different approaches to CPU scheduling. The interesting differences aren't just *which thread runs next*, but also:

- **Priority:** Who is entitled to run before whom?
- **Time budget:** How much CPU time is a thread entitled to consume?
- **Preemption:** When can another thread take over?
- **Fairness:** How is CPU time distributed among competing threads?
- **Latency:** How quickly can a newly runnable thread begin executing?
- **Isolation:** Can one thread exhaust CPU resources needed by another?
- **Admission control:** Can the system guarantee that requested CPU budgets are actually available?

The distinction between **priority** and **budget** is particularly important. Many traditional schedulers treat priority as a way to influence CPU allocation. More sophisticated real-time and capability-based systems treat priority and budget as separate resources.

I'll compare Linux, Windows, macOS, FreeBSD, seL4, QNX, and several real-time scheduling models, with particular attention to how their mechanisms work internally.

## 1. The fundamental scheduling models

Before comparing operating systems, it's useful to distinguish five major families.

| Model | Scheduling decision | Budget mechanism | Typical use |
|---|---|---|---|
| Fixed-priority preemptive | Highest-priority runnable thread | Optional quantum or replenished budget | RTOS, kernel threads |
| Round-robin | Rotate among equal-priority threads | Fixed quantum | General-purpose and RTOS |
| Fair-share | Thread with lowest accumulated service | Virtual runtime or equivalent | Desktop/server workloads |
| Deadline-based | Thread with earliest scheduling deadline | Runtime reservation | Hard/soft real-time |
| Feedback/heuristic | Dynamically adjusted priority or urgency | Quantum, decay, usage history | Interactive workloads |

These aren't mutually exclusive. An OS commonly implements several scheduling classes with a strict precedence relationship between them.

### Priority versus budget

Consider two threads:

- **A:** priority 10, budget 2 ms per 10 ms
- **B:** priority 5, budget 8 ms per 10 ms

If both are runnable, A executes first, but after consuming 2 ms, it becomes ineligible until its budget is replenished.

B can then execute.

```text
Time (ms)
0         2                         10
|---------|--------------------------|
|    A    |             B            |
|---------|--------------------------|

A: priority 10, budget 2/10 ms
B: priority  5, budget 8/10 ms
```

This is fundamentally different from traditional priority scheduling, where A could monopolize the CPU indefinitely as long as it remains runnable.

**A priority determines who runs first. A budget determines how long they are allowed to run.**

That distinction is central to understanding seL4's MCS scheduler, Linux SCHED_DEADLINE, and QNX adaptive partitioning.

---

# 2. Linux

Linux is interesting because it effectively contains several schedulers, organized into scheduling classes.

### Scheduling classes

In simplified precedence order:

| Class | Policy | Priority | CPU budget |
|---|---|---|---|
| Deadline | `SCHED_DEADLINE` | Deadline-based | Explicit runtime/period |
| Real-time | `SCHED_FIFO` | Static RT priority | Normally unlimited |
| Real-time | `SCHED_RR` | Static RT priority | Quantum, but no periodic budget |
| Fair | `SCHED_OTHER` | Weight derived from nice | Fair-share allocation |
| Fair | `SCHED_BATCH` | Weight derived from nice | Fair-share allocation |
| Fair | `SCHED_IDLE` | Very low scheduling precedence | Opportunistic CPU use |
| Idle | Kernel idle task | Lowest | N/A |

Linux also has `SCHED_EXT`, which allows scheduling policies to be implemented using BPF.

## 2.1 CFS and EEVDF: fair scheduling

Linux used the Completely Fair Scheduler (CFS) for many years. Starting with Linux 6.6, its fair scheduling class began transitioning to **EEVDF — Earliest Eligible Virtual Deadline First**.

The underlying goal remains proportional CPU allocation.

Suppose three CPU-bound threads have equal weights.

Ideally:

```text
Thread A: 33.3% CPU
Thread B: 33.3% CPU
Thread C: 33.3% CPU
```

Neither CFS nor EEVDF needs to maintain an explicit periodic CPU budget for each thread.

Instead, they account for how much CPU service each thread has received.

### CFS: virtual runtime

CFS maintained a quantity approximately equivalent to:

\[
\Delta v = \Delta t \cdot \frac{w_0}{w}
\]

Where:

- \(v\) is virtual runtime.
- \(t\) is actual execution time.
- \(w\) is the thread's scheduling weight.
- \(w_0\) is a reference weight.

A thread with twice the weight accumulates virtual runtime at half the rate.

CFS generally selected the runnable thread with the smallest virtual runtime.

This naturally distributes CPU time according to weights.

### EEVDF: virtual deadlines

EEVDF introduces two important concepts:

1. **Lag:** How much CPU service a thread is owed relative to its fair allocation.
2. **Virtual deadline:** When the thread should receive its next portion of service in virtual time.

It chooses the earliest virtual deadline among eligible threads.

This improves the ability to distinguish latency-sensitive tasks from CPU-intensive tasks while maintaining long-term fairness.

A short requested slice can result in a short virtual deadline, allowing interactive tasks to run sooner.

**Important:** EEVDF virtual deadlines are not hard real-time deadlines.

They are accounting constructs used to implement fairness and latency preferences.

### Where do budgets come from?

For ordinary Linux threads, CPU budgets are mostly implicit.

A thread's entitlement is determined by its weight relative to other runnable threads:

\[
\text{CPU share}_i =
\frac{w_i}{\sum_j w_j}
\]

For explicit limits, Linux provides cgroup CPU bandwidth control.

For example:

```text
cpu.max = 20000 100000
```

This limits a cgroup to 20 ms of CPU time per 100 ms period, equivalent to 20% of one CPU.

On multicore systems, the quota is aggregate CPU time across CPUs.

This is an actual enforced bandwidth budget, unlike a nice value.

## 2.2 SCHED_FIFO and SCHED_RR

These are traditional fixed-priority real-time policies.

Linux provides RT priorities 1–99, with 99 highest.

`SCHED_FIFO` behaves as follows:

- The highest-priority runnable thread executes.
- A higher-priority thread immediately preempts it.
- Equal-priority threads generally do not preempt each other simply because time passes.
- A thread continues until it blocks, yields, or is preempted.

`SCHED_RR` adds a quantum for rotation among threads of equal priority.

However, **the RR quantum is not a CPU budget**.

A single runnable `SCHED_RR` thread can still consume 100% of a CPU.

Linux does provide RT bandwidth throttling, controlled through mechanisms such as:

```text
/proc/sys/kernel/sched_rt_period_us
/proc/sys/kernel/sched_rt_runtime_us
```

Common defaults reserve 5% of CPU bandwidth for non-RT tasks by limiting RT execution to 950 ms per 1-second period.

These are aggregate controls, not individual thread reservations.

## 2.3 SCHED_DEADLINE

This is Linux's most interesting scheduler from a budget-management perspective.

It combines:

- Earliest Deadline First (EDF)
- Constant Bandwidth Server (CBS)

Each thread requests three parameters:

```text
runtime  =  2 ms
deadline =  8 ms
period   = 10 ms
```

Meaning:

> Give this thread up to 2 ms of CPU execution per 10 ms period, with a scheduling deadline 8 ms after the start of each period.

The scheduler tracks remaining runtime and an absolute scheduling deadline.

A simplified timeline:

```text
Period 1                         Period 2
0                                10 ms
|--------------------------------|
[ A: 2 ms ]                      [ A: 2 ms ]
         ^ deadline at 8 ms
```

The utilization reservation is:

\[
U = \frac{Q}{P}
\]

Where:

- \(Q\) is reserved runtime.
- \(P\) is period.

For this thread:

\[
U = \frac{2}{10} = 0.2
\]

It reserves 20% of one CPU.

Linux performs admission control to avoid accepting reservations that exceed its configured capacity constraints.

CBS prevents a thread from monopolizing the CPU when it exceeds its reserved runtime. Runtime depletion results in throttling until replenishment.

An important distinction is that a deadline reservation provides scheduling guarantees only under the relevant schedulability assumptions. Admission does not guarantee arbitrary application-level deadlines will be met.

### Linux summary

Linux has three quite different resource models:

- **Fair:** Proportional entitlement, tracked through virtual time.
- **RT:** Strict priority, generally without individual budget enforcement.
- **Deadline:** Explicit periodic CPU reservations with bandwidth enforcement.

This combination makes Linux unusually flexible, although scheduling behavior across classes can be complex.

---

# 3. seL4: capability-controlled CPU time

seL4 is particularly interesting because its MCS (Mixed-Criticality Systems) extensions make CPU time an explicitly controlled kernel resource.

There are two relevant scheduling models.

## 3.1 Traditional seL4

The original scheduler is straightforward:

- Fixed priorities, typically 0–255.
- Highest-priority runnable thread wins.
- Round-robin among equal-priority threads.
- Each thread has a timeslice.
- No general per-thread periodic CPU reservation.

This resembles traditional RTOS scheduling.

A high-priority CPU-bound thread can starve lower-priority threads.

## 3.2 seL4 MCS: scheduling contexts

MCS introduces a separate kernel object:

```text
Thread Control Block
    |
    | bound to
    v
Scheduling Context
    |
    +-- Budget: 2 ms
    +-- Period: 10 ms
    +-- Replenishment queue
```

A thread needs an eligible scheduling context to execute.

The scheduling context specifies an upper bound on CPU consumption:

\[
U_{\max} = \frac{\text{budget}}{\text{period}}
\]

The kernel still chooses the highest-priority eligible runnable thread.

The crucial difference is that **a high-priority thread can run out of CPU time and become temporarily ineligible**.

This gives the system temporal isolation even when priorities are unfavorable.

### Sporadic server replenishment

Unlike a simple periodic scheduler that resets the entire budget every 10 ms, seL4 MCS uses a sporadic-server mechanism.

Consider:

```text
Budget = 3 ms
Period = 10 ms
```

The thread executes:

```text
Time:  0       1       3       5       10      13
       |-------|-------|-------|-------|-------|
Run:   [1 ms]          [2 ms]
```

The scheduler records replenishments approximately as follows:

```text
Consumed 1 ms at t=0:
    replenish 1 ms at t=10

Consumed 2 ms at t=3:
    replenish 2 ms at t=13
```

This prevents a thread from consuming its full budget at the end of one period and immediately consuming another full budget at the beginning of the next.

That phenomenon is called *budget bunching*.

Sporadic replenishment provides a stronger bound on CPU consumption over time than a naive periodic reset.

Actual seL4 accounting involves refill records, eligibility timestamps, and rules for merging refills when the queue reaches its configured capacity. [seL4 Documentation](https://docs.sel4.systems/Tutorials/mcs.html?utm_source=chatgpt.com)

### Scheduling context donation

One of seL4 MCS's most interesting features is that scheduling contexts are independent of threads.

Consider:

```text
Client A ──RPC──> Server S
Client B ──RPC──> Server S
```

A traditional system might give S its own CPU budget.

But this introduces a problem: whose CPU time should be charged for processing A's request?

MCS allows **passive servers**.

A passive server does not have a scheduling context of its own while waiting for requests.

Instead:

```text
Client A:
    Priority = 100
    Budget   = 2 ms

Server S:
    Priority = 120
    Budget   = none

A calls S
    |
    v
S executes using A's scheduling context
    |
    v
CPU consumption charged to A
```

When S replies, the scheduling context returns along the IPC chain.

This makes it possible to isolate clients from each other's CPU consumption even when they share the same server.

It also separates two forms of authority:

- Authority to execute at a particular priority.
- Authority to consume a particular amount of CPU time.

Importantly, donation of a scheduling context does not automatically mean donation of the caller's priority. The server's effective priority is governed separately.

### What seL4 does not guarantee

An important subtlety:

**A budget is an upper bound, not a guaranteed minimum allocation.**

Suppose:

```text
A: priority 200, budget 8/10 ms
B: priority 100, budget 5/10 ms
```

Both configurations are individually valid, but the combined requested utilization is 130%.

The scheduler cannot guarantee that both receive their full budgets.

In particular, B can be deprived of its desired execution time by A.

The kernel enforces consumption limits, but schedulability analysis and resource allocation remain the responsibility of the system designer.

This differs from Linux SCHED_DEADLINE, which includes admission control.

The seL4 manual explicitly distinguishes the CPU upper bound from a lower-bound execution guarantee. [seL4](https://www.sel4.com/Info/Docs/seL4-manual-latest.pdf?utm_source=chatgpt.com)

### Why this design is important

seL4 effectively turns CPU execution time into something resembling a capability-controlled resource.

The kernel enforces the maximum consumption, while higher-level components can implement policies such as:

- Periodic reservations.
- Sporadic execution.
- Temporal isolation between components.
- Budget donation for RPC.
- User-level scheduling and resource management.

This is particularly attractive for microkernels because it allows CPU resource management to follow the same delegation principles as memory and IPC authority.

---

# 4. Windows: dynamic priority scheduling

Windows uses a priority-driven, preemptive scheduler, but with considerably more dynamic behavior than a traditional RTOS.

## 4.1 Priority structure

Windows has 32 scheduling priority levels:

| Priority | Meaning |
|---|---|
| 0 | Special system priority |
| 1–15 | Variable-priority range |
| 16–31 | Real-time range |

A thread's base priority depends on its process priority class and thread-relative priority.

Windows can temporarily boost the priorities of variable-priority threads in response to events such as I/O completion.

This helps interactive threads run promptly after waking.

For example:

```text
CPU-bound thread:
    base priority 8

Interactive thread:
    base priority 8
    receives I/O completion
    temporarily boosted to 12
```

The interactive thread preempts the CPU-bound thread.

Its priority subsequently decays toward its base level as it executes.

## 4.2 Quantums

Windows assigns execution quantums to threads.

Threads at the same priority generally share CPU time through quantum-based rotation.

When a thread exhausts its quantum, it may be preempted by another runnable thread.

However, this quantum is not a periodic CPU reservation.

A high-priority thread can continue consuming CPU if no equally or more highly prioritized thread needs it.

## 4.3 Budget enforcement

Traditional Windows thread scheduling does not expose a Linux SCHED_DEADLINE-style `(runtime, deadline, period)` interface.

Instead, Windows provides several other resource-management mechanisms.

**Job Objects** can enforce CPU usage limits on groups of processes.

For example, a job can be configured with a hard CPU rate limit.

Windows also provides CPU rate weighting and processor affinity controls.

These operate at a different level from the thread scheduler's dynamic priority and quantum mechanisms.

### Windows scheduling philosophy

Windows primarily optimizes for:

- Responsiveness.
- Throughput.
- Dynamic priority adjustments.
- Avoiding unnecessary starvation of ordinary workloads.

It is less oriented toward explicit per-thread real-time execution contracts.

---

# 5. macOS and Darwin: priority plus QoS

Darwin's scheduler combines traditional priority scheduling with application-level Quality of Service (QoS) information.

The central idea is that the system should understand the *importance of the work*, rather than relying solely on numeric priorities.

## 5.1 QoS classes

macOS exposes several QoS categories:

| QoS | Intended use |
|---|---|
| User Interactive | UI interaction, animations |
| User Initiated | Work initiated by the user |
| Default | Ordinary application work |
| Utility | Longer-running, lower-urgency work |
| Background | Maintenance, indexing, background operations |

These influence scheduling priority and other system resource policies.

For example:

```text
UI thread:
    QoS = User Interactive

Image processing:
    QoS = User Initiated

Background indexing:
    QoS = Background
```

The scheduler favors latency-sensitive work over background computation.

Darwin also maintains internal scheduling priority bands and supports real-time scheduling constraints.

## 5.2 Time constraints

Darwin provides a time-constraint policy through Mach:

```c
thread_time_constraint_policy_data_t
```

It contains:

```c
period
computation
constraint
preemptible
```

Conceptually:

- `period`: Expected interval between activations.
- `computation`: Expected CPU execution time.
- `constraint`: Completion-time constraint.
- `preemptible`: Whether the thread can be preempted under the policy.

This resembles a reservation interface, although Darwin's implementation and guarantees are not equivalent to Linux SCHED_DEADLINE.

In particular, it should not be interpreted as a general hard real-time guarantee.

macOS uses this functionality for latency-sensitive workloads, including audio processing.

## 5.3 Priority propagation

Darwin also has mechanisms for propagating scheduling importance through synchronization and work dependencies.

For example, when high-priority work depends on a lower-priority thread, the system may temporarily increase the effective priority of the thread performing the required work.

This is related to priority inheritance, although Darwin's broader QoS propagation mechanisms operate at several levels.

### macOS scheduling philosophy

Darwin emphasizes:

- Interactive responsiveness.
- Energy efficiency.
- Workload classification.
- Priority propagation.
- Special handling for latency-sensitive work.

Unlike seL4 MCS, ordinary applications do not receive independent, capability-controlled CPU budgets.

---

# 6. FreeBSD: ULE and traditional real-time scheduling

FreeBSD's default scheduler is **ULE**, which combines priority scheduling, interactivity heuristics, and CPU affinity.

It also has a strong focus on multicore scheduling.

## 6.1 ULE

ULE maintains per-CPU scheduling queues and attempts to balance work between processors.

It distinguishes between interactive and CPU-intensive threads using their recent behavior.

For example:

```text
Thread A:
    Frequently sleeps waiting for input
    → Interactive

Thread B:
    Continuously consumes CPU
    → CPU-intensive
```

Interactive threads receive favorable scheduling treatment to reduce latency.

ULE also tries to preserve cache locality by avoiding unnecessary thread migrations.

Unlike Linux's fair scheduler, ULE is not fundamentally organized around a single virtual-runtime fairness metric.

## 6.2 Priorities

FreeBSD supports multiple scheduling priority classes, including real-time, timeshare, and idle.

The timeshare class uses dynamic priorities.

Real-time threads receive stronger scheduling precedence and can starve ordinary threads if improperly configured.

FreeBSD also exposes POSIX scheduling policies such as `SCHED_FIFO` and `SCHED_RR`.

## 6.3 Budgets

ULE doesn't generally associate periodic CPU budgets with individual threads.

Instead, it uses:

- Timeslices.
- Dynamic priorities.
- CPU usage accounting.
- Resource limits.
- CPU affinity and processor sets.

FreeBSD also provides `rctl` for resource control, including CPU usage constraints, although these are not equivalent to hard real-time scheduling reservations.

**FreeBSD is closer to traditional Unix scheduling than to the explicit budget-based model of seL4 MCS.**

---

# 7. QNX: fixed priorities with adaptive partitioning

QNX Neutrino is particularly interesting because it combines traditional real-time scheduling with an optional resource-partitioning mechanism.

## 7.1 Traditional QNX scheduling

QNX primarily uses fixed-priority preemptive scheduling.

It supports:

- `SCHED_FIFO`
- `SCHED_RR`
- `SCHED_SPORADIC`

The last one is especially interesting.

### Sporadic scheduling

A sporadic server is intended to provide predictable execution capacity to a thread that wakes intermittently.

For example:

```text
High priority: 20
Low priority:  5

Initial budget: 2 ms
Replenishment period: 10 ms
```

The thread initially executes at priority 20.

After exhausting its available high-priority budget, it drops to priority 5 until its budget is replenished.

This differs from seL4 MCS.

In seL4 MCS, an exhausted scheduling context makes its thread ineligible to execute until replenishment.

In QNX's sporadic scheduling policy, the thread can continue executing at a lower priority.

This distinction matters:

| | seL4 MCS | QNX sporadic scheduling |
|---|---|---|
| Budget exhaustion | Thread becomes ineligible | Thread drops to low priority |
| CPU usage strictly capped? | Yes, by scheduling context | No, low-priority execution remains possible |
| Replenishment | Sporadic | Sporadic |
| Primary purpose | Temporal isolation | Real-time responsiveness with graceful degradation |

## 7.2 Adaptive Partitioning Scheduler (APS)

QNX also provides an Adaptive Partitioning Scheduler.

Rather than assigning budgets to individual threads, APS assigns CPU bandwidth to **partitions**.

For example:

```text
Partition A: Control software  50%
Partition B: Device drivers    30%
Partition C: Diagnostics       20%
```

Threads belong to partitions, and each partition has a CPU budget.

The scheduler combines:

1. Thread priorities.
2. Partition CPU consumption.
3. Partition bandwidth reservations.

### What makes it adaptive?

Suppose diagnostics is idle.

```text
Control:      wants 70%
Drivers:      wants 30%
Diagnostics:  wants  0%
```

Instead of leaving 20% of the CPU unused, APS can allow control to consume the spare capacity.

But if diagnostics becomes runnable and needs its reserved bandwidth, the scheduler can restrict the other partitions to preserve the reservation.

The bandwidth accounting is based on an averaging window rather than simply resetting all partitions' budgets at fixed period boundaries.

This makes APS a form of **work-conserving resource partitioning**: unused capacity can be borrowed, subject to the scheduler's reservation rules.

QNX documents the conditions under which partitions receive their guaranteed bandwidth, including restrictions related to multicore capacity and critical-time execution. [QNX](https://www.qnx.com/developers/docs/6.4.0/adaptive_partitioning_en/user_guide/faq_ap.html?utm_source=chatgpt.com)

### Why APS is interesting

APS separates scheduling into two levels:

```text
Global scheduler
    |
    +-- Partition A: 50%
    |       +-- Thread A1: priority 30
    |       +-- Thread A2: priority 20
    |
    +-- Partition B: 30%
    |       +-- Thread B1: priority 40
    |       +-- Thread B2: priority 10
    |
    +-- Partition C: 20%
            +-- Thread C1: priority 15
```

Priorities determine urgency, while partition budgets constrain resource consumption.

This resembles hierarchical scheduling, where one scheduler allocates resources to groups and another determines execution order within each group.

---

# 8. Other notable scheduling designs

Several additional systems are worth mentioning because they illustrate approaches that differ from mainstream Unix scheduling.

## 8.1 Zephyr: configurable embedded scheduling

Zephyr is a configurable RTOS that supports cooperative and preemptive threads.

Its scheduling model is relatively simple:

- Numeric thread priorities.
- Cooperative threads that retain the CPU until they yield or block.
- Preemptive threads scheduled according to priority.
- Optional timeslicing among equal-priority threads.
- Optional deadline-related scheduling features.

Zephyr's ordinary timeslicing is not a CPU reservation mechanism.

It generally leaves application-level periodic execution and budget enforcement to higher-level logic.

This is common in embedded RTOS designs: keep the scheduler small and predictable, and let the application engineer ensure schedulability.

## 8.2 RTEMS: multiple scheduling algorithms

RTEMS is notable for supporting different scheduler implementations.

Depending on configuration, these include:

- Fixed-priority scheduling.
- Earliest Deadline First.
- Scheduling variants designed for symmetric multiprocessing.

RTEMS demonstrates an alternative philosophy: rather than implementing one sophisticated general-purpose policy, an embedded system can select a scheduler appropriate for its workload.

## 8.3 Xen: scheduling virtual CPUs

Hypervisors introduce another level of scheduling.

Xen has supported several scheduling policies, including Credit, Credit2, and RTDS.

The interesting one here is **RTDS**, a real-time scheduler using budget and period parameters for virtual CPUs.

For example:

```text
VM A / vCPU 0:
    Budget = 4 ms
    Period = 10 ms

VM B / vCPU 0:
    Budget = 6 ms
    Period = 10 ms
```

This resembles thread-level reservations, except the scheduled entities are virtual CPUs.

There is an important complication: the guest OS has its own scheduler.

Consequently, end-to-end scheduling behavior depends on both:

```text
Hypervisor scheduling
        ↓
Guest OS scheduling
        ↓
Application thread
```

A thread's deadline can be missed even if the guest scheduler behaves correctly, because its virtual CPU might not be executing.

This motivates hierarchical real-time scheduling and compositional schedulability analysis.

---

# 9. The deeper distinction: four meanings of a time budget

One of the most confusing aspects of OS scheduling terminology is that *budget* can mean several fundamentally different things.

## 9.1 Quantum: when to reconsider scheduling

A quantum is simply the maximum continuous execution interval before the scheduler reconsideres its choice.

```text
Quantum = 5 ms
```

After 5 ms, the thread may be moved to the back of its priority queue.

But it may immediately run again.

**A quantum does not limit total CPU consumption.**

Examples: Windows timeslices, Linux `SCHED_RR`, traditional seL4.

## 9.2 Quota: an upper bound on CPU consumption

A quota restricts the CPU time consumed during some interval.

```text
Quota = 20 ms
Period = 100 ms
```

Once the quota is exhausted, execution is throttled.

This provides isolation from excessive CPU consumption.

Examples: Linux cgroup CPU quotas, seL4 MCS scheduling contexts.

However, a quota by itself does not guarantee the thread will receive that much execution time.

## 9.3 Reservation: an entitlement to CPU time

A reservation promises access to some amount of CPU capacity, provided the relevant assumptions hold.

```text
Runtime  = 2 ms
Period   = 10 ms
Deadline = 8 ms
```

The scheduler must account for competing reservations.

This usually requires admission control or offline schedulability analysis.

Example: Linux `SCHED_DEADLINE`.

**A reservation is a stronger contract than a quota.**

A quota says:

> You cannot use more than this.

A reservation says:

> Under the scheduling assumptions, this much capacity is available to you.

## 9.4 Share: a relative entitlement

A share describes how CPU capacity should be distributed under contention.

```text
A: weight 100
B: weight 200
```

When both are runnable:

```text
A gets approximately 33%
B gets approximately 67%
```

But when B sleeps, A may use the entire CPU.

Examples: Linux EEVDF, CPU weight controls.

This is neither a strict quota nor a guaranteed real-time reservation.

### Comparison

| Property | Quantum | Quota | Reservation | Share |
|---|---|---|---|---|
| Limits continuous execution | Yes | Sometimes | Sometimes | Indirectly |
| Limits total CPU usage | No | Yes | Typically enforced | No |
| Guarantees CPU allocation | No | No | Conditionally | Relative fairness |
| Requires replenishment | No | Yes | Yes | No |
| Work-conserving by default | Yes | Not necessarily | Depends on policy | Yes |
| Supports temporal isolation | No | Yes | Yes | Limited |

This distinction is more important than the specific names operating systems give their scheduling policies.

---

# 10. Priority inversion and budget inheritance

Once priorities and budgets are separated, another interesting problem appears.

Suppose:

```text
Client A:
    Priority = 200
    Budget   = 2 ms

Server S:
    Priority = 100
    Budget   = 5 ms

Background B:
    Priority = 150
```

A sends a synchronous request to S.

A blocks waiting for the reply.

But B has higher priority than S, so B can prevent S from executing.

A is indirectly blocked by a lower-priority server.

This is **priority inversion**.

Traditional systems address this with priority inheritance or priority ceiling protocols.

But with explicit CPU budgets, we have two independent questions:

1. At what priority should S execute?
2. Whose CPU budget should S consume?

These produce four conceptual combinations:

| Priority | Budget | Interpretation |
|---|---|---|
| Server | Server | Ordinary independent server |
| Client | Server | Priority inheritance |
| Server | Client | Budget donation |
| Client | Client | Priority and budget inheritance |

These choices have different implications.

### Priority inheritance alone

S executes at A's priority, but consumes its own budget.

This solves priority inversion but can allow one client to exhaust a shared server's budget.

### Budget donation alone

S consumes A's budget, but executes at its own priority.

This isolates CPU accounting but does not necessarily solve priority inversion.

### Both

S executes at A's effective priority using A's budget.

This is often the most intuitive model for synchronous RPC, but it requires careful handling of nested calls, cycles, exhaustion, and restoration of scheduling state.

For example:

```text
A → S1 → S2 → S3
```

If the budget propagates through the entire call chain, CPU consumption by S3 is ultimately charged to A.

This is a powerful property for microkernel architectures, where ordinary OS services are implemented as userspace servers.

However, the exact semantics vary between kernels. seL4 MCS provides scheduling-context donation, not unrestricted automatic priority inheritance.

---

# 11. Fixed-priority scheduling versus EDF

There's a deeper algorithmic distinction worth exploring.

## Rate Monotonic Scheduling (RMS)

Rate Monotonic Scheduling assigns fixed priorities based on periods.

Shorter-period tasks receive higher priorities.

Consider:

| Thread | Execution | Period | Priority |
|---|---:|---:|---|
| A | 1 ms | 4 ms | High |
| B | 2 ms | 5 ms | Medium |
| C | 2 ms | 10 ms | Low |

Their total utilization is:

\[
U = \frac14 + \frac25 + \frac2{10} = 0.85
\]

For three independent periodic tasks with deadlines equal to periods, the classical sufficient RMS utilization bound is:

\[
U \leq 3(2^{1/3}-1) \approx 0.7798
\]

Our task set exceeds that bound.

This does **not** mean it is necessarily unschedulable. The bound is sufficient, not necessary.

More precise response-time analysis can establish schedulability for task sets that exceed it.

## Earliest Deadline First (EDF)

EDF dynamically prioritizes whichever job has the nearest absolute deadline.

For independent, preemptible periodic tasks on a single processor, with deadlines equal to periods and negligible scheduling overhead, EDF can schedule task sets with:

\[
U \leq 1
\]

This is a major theoretical advantage.

However, EDF has tradeoffs.

With fixed priorities, overload behavior is often relatively predictable: high-priority tasks remain favored, while lower-priority tasks miss deadlines.

Under EDF, overload can affect a broader set of tasks.

This is one reason fixed-priority scheduling remains attractive for safety-critical systems.

### A crucial qualification

Neither RMS nor EDF guarantees deadlines solely because a task has a budget.

A complete analysis may need to account for:

- Worst-case execution time (WCET).
- Blocking on locks and IPC.
- Interrupt interference.
- Kernel execution time.
- Cache effects.
- Multicore interference.
- Release jitter.
- Priority inversion.

A CPU reservation is one component of a real-time guarantee, not the entire guarantee.

---

# 12. Overall comparison

Here's the condensed comparison across the systems we've discussed.

| OS / Scheduler | Priority model | Budget model | Exhaustion behavior | Main objective |
|---|---|---|---|---|
| Linux EEVDF | Virtual deadlines, weights | Proportional share | No periodic exhaustion | Fairness and latency |
| Linux FIFO | Fixed priority | None by default | N/A | RT responsiveness |
| Linux RR | Fixed priority | Quantum | Rotate among peers | RT time sharing |
| Linux DEADLINE | EDF | Runtime/period | Throttle/replenish | Real-time reservations |
| Windows | Dynamic priorities | Quantum | Reschedule | Responsiveness |
| macOS | QoS, priorities | Quantum / RT constraints | Policy-dependent | Latency and energy |
| FreeBSD ULE | Dynamic priorities | Timeslice | Reschedule | Interactive throughput |
| seL4 traditional | Fixed priority | Timeslice | Rotate among peers | Simple RT scheduling |
| seL4 MCS | Fixed priority | Sporadic budget/period | Ineligible until refill | Temporal isolation |
| QNX FIFO/RR | Fixed priority | None / quantum | Continue / rotate | Deterministic RT |
| QNX sporadic | Fixed priority with fallback | Sporadic high-priority budget | Drop priority | RT responsiveness |
| QNX APS | Priority + partition | Partition bandwidth | Partition constrained | CPU isolation |
| Zephyr | Fixed priority | Optional timeslice | Rotate among peers | Embedded RT |
| RTEMS EDF | Dynamic deadline | Task execution requirements | Policy-dependent | Embedded RT |
| Xen RTDS | Deadline-based | vCPU budget/period | Replenishment | VM-level RT |

## 13. What I find most interesting architecturally

Three approaches stand out.

### Linux: scheduling as an optimization problem

Linux's fair scheduler treats CPU time as a continuously distributed resource.

The scheduler tries to optimize fairness and latency while adapting to the current workload.

It doesn't require applications to know their CPU requirements beforehand.

This is excellent for general-purpose systems but offers limited predictability for individual threads.

### seL4 MCS: scheduling as resource authority

seL4 treats CPU execution capacity as an explicitly managed resource.

The kernel provides a relatively small mechanism:

```text
(priority, scheduling context)
```

The scheduling context defines the permitted execution budget, while priority determines precedence.

Higher-level scheduling policy can be implemented outside the kernel.

This is conceptually elegant because it separates mechanism from policy and supports explicit CPU resource delegation.

But it also pushes responsibility for schedulability and resource allocation onto the system designer.

### QNX APS: scheduling as resource partitioning

QNX APS takes a more system-oriented approach.

Instead of requiring every thread to have its own budget, it reserves CPU capacity for groups of threads.

Within those groups, ordinary priority scheduling remains useful.

This is particularly attractive for systems containing multiple subsystems with different criticality requirements.

---

## 14. Three questions that expose a scheduler's real semantics

When examining an unfamiliar scheduler, I would start with these questions.

**First: What happens when a thread exhausts its quantum or budget?**

Does it rotate, become ineligible, drop priority, or simply accumulate scheduling debt?

These produce fundamentally different guarantees.

**Second: Does the scheduler enforce only upper bounds, or does it guarantee minimum service?**

This distinguishes CPU isolation from CPU reservations.

For example, seL4 MCS enforces upper bounds, while Linux SCHED_DEADLINE combines bandwidth enforcement with admission-controlled reservations. [seL4 Documentation](https://docs.sel4.systems/Tutorials/mcs.html?utm_source=chatgpt.com)

**Third: Who pays for CPU time spent servicing another thread?**

This is particularly important in microkernels.

If a client calls a filesystem server, network server, or device driver, does that execution consume the client's budget or the server's?

The answer determines whether shared services can provide temporal isolation between their clients.

## Further reading

For the most technically interesting designs, I'd prioritize these:

1. **[seL4 MCS documentation](https://docs.sel4.systems/Tutorials/mcs.html)** — scheduling contexts, sporadic replenishment, passive servers, and CPU-time donation.
2. **[Linux SCHED_DEADLINE documentation](https://www.kernel.org/doc/html/latest/scheduler/sched-deadline.html)** — EDF, Constant Bandwidth Server, replenishment, and admission control.
3. **[Linux EEVDF documentation](https://kernel.org/doc/html/latest/scheduler/sched-eevdf.html)** — virtual deadlines, lag, and proportional fairness.
4. **[seL4 whitepaper](https://www.sel4.com/About/seL4-whitepaper.pdf)** — particularly the discussion of temporal isolation and scheduling-context capabilities.
5. **[QNX Adaptive Partitioning documentation](https://www.qnx.com/developers/docs/6.4.0/adaptive_partitioning_en/user_guide/faq_ap.html)** — bandwidth guarantees and partition behavior.

**The biggest conceptual divide is between schedulers that distribute CPU time according to observed demand and schedulers that enforce explicitly allocated execution capacity.**

Linux EEVDF and Windows largely belong to the first category. seL4 MCS and Linux SCHED_DEADLINE belong to the second, although they differ substantially in their guarantees.

QNX APS occupies an interesting middle ground: explicit resource partitioning combined with adaptive use of otherwise idle CPU capacity.
