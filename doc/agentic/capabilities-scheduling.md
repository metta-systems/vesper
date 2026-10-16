# OS scheduling policies: priorities, budgets, deadlines, and fairness

Operating systems take substantially different approaches to CPU scheduling. The most interesting differences aren't merely *which thread runs next*, but how scheduling priority interacts with CPU-time accounting.

There are six questions worth asking about every scheduler:

1. **Priority:** Which runnable thread gets to execute first?
2. **Time budget:** How much CPU time can a thread consume?
3. **Preemption:** When can another thread take over?
4. **Fairness:** How is CPU capacity distributed among competing threads?
5. **Latency:** How quickly can a newly runnable thread begin executing?
6. **Isolation:** Can one thread consume CPU capacity intended for another?

A seventh question becomes important for real-time systems: **Does the scheduler merely enforce CPU limits, or can it guarantee CPU availability?**

We'll compare Linux (EEVDF, FIFO, RR, DEADLINE), **Con Kolivas's MuQSS**, Windows, macOS, FreeBSD, seL4, QNX, Zephyr, RTEMS, and Xen.

## 1. Fundamental scheduling models

Most schedulers combine several of these approaches:

| Model | Scheduling decision | CPU-time accounting | Typical use |
|---|---|---|---|
| Fixed-priority preemptive | Highest-priority runnable thread | Optional quantum or budget | RTOS, kernel threads |
| Round-robin | Rotate equal-priority threads | Fixed quantum | General-purpose and RTOS |
| Fair-share | Lowest accumulated service or equivalent | Virtual runtime, weights | Desktop/server |
| Deadline-based | Earliest deadline | Runtime reservation or virtual deadline | Real-time and interactive |
| Feedback-based | Dynamically adjusted priority | CPU usage, sleep history | Interactive workloads |
| Partition-based | Priority subject to partition eligibility | Group bandwidth allocation | Mixed-criticality systems |

These approaches are not mutually exclusive.

In particular, **deadline scheduling doesn't necessarily imply real-time guarantees**. Both Linux EEVDF and MuQSS use deadlines, but their deadlines serve different purposes from those of an admission-controlled real-time scheduler.

### Priority versus budget

Consider two threads:

| Thread | Priority | Budget | Period |
|---|---:|---:|---:|
| A | 10 | 2 ms | 10 ms |
| B | 5 | 8 ms | 10 ms |

If both are runnable, A executes first. Once A consumes its 2 ms budget, it becomes ineligible until replenishment.

```text
Time (ms)
0         2                         10
|---------|--------------------------|
|    A    |             B            |
|---------|--------------------------|
```

This is fundamentally different from traditional fixed-priority scheduling, where A could monopolize the CPU indefinitely.

**Priority determines who runs first. Budget determines how much execution is permitted.**

That distinction is central to seL4 MCS, Linux SCHED_DEADLINE, and QNX adaptive partitioning.

---

# 2. Linux: multiple scheduling classes

Linux implements several scheduling classes, with a precedence relationship between them.

| Class | Policy | Priority | Budget |
|---|---|---|---|
| Deadline | `SCHED_DEADLINE` | Dynamic deadline | Runtime/period |
| Real-time | `SCHED_FIFO` | Static RT priority | Normally unlimited |
| Real-time | `SCHED_RR` | Static RT priority | Quantum, not periodic budget |
| Fair | `SCHED_OTHER` | Weight from nice | Proportional CPU share |
| Fair | `SCHED_BATCH` | Weight from nice | Proportional CPU share |
| Fair | `SCHED_IDLE` | Very low precedence | Opportunistic CPU |
| Idle | Kernel idle task | Lowest | N/A |

Linux also supports `SCHED_EXT`, allowing BPF-based scheduling policies.

## 2.1 CFS and EEVDF

Linux historically used the Completely Fair Scheduler (CFS), with its fair scheduling class transitioning to **EEVDF — Earliest Eligible Virtual Deadline First** beginning in Linux 6.6.

The underlying objective is proportional CPU allocation.

Suppose three CPU-bound threads have equal weights:

```text
A: 33.3%
B: 33.3%
C: 33.3%
```

Neither CFS nor EEVDF requires an explicit periodic CPU budget for every thread.

Instead, the scheduler accounts for CPU service received.

### CFS: virtual runtime

CFS used virtual runtime:

\[
\Delta v = \Delta t \cdot \frac{w_0}{w}
\]

Where:

- \(v\) is virtual runtime.
- \(t\) is actual execution time.
- \(w\) is scheduling weight.
- \(w_0\) is a reference weight.

A thread with twice the weight accumulates virtual runtime at half the rate.

The scheduler generally selected the runnable thread with the smallest virtual runtime.

### EEVDF: virtual deadlines

EEVDF introduces two key concepts:

- **Lag:** How much CPU service a thread is owed relative to its fair allocation.
- **Virtual deadline:** A scheduling deadline calculated in virtual time.

The scheduler selects the earliest virtual deadline among eligible threads.

This makes it possible to distinguish latency-sensitive threads from CPU-intensive threads while preserving long-term fairness.

Shorter requested slices generally produce earlier virtual deadlines.

However, EEVDF deadlines are **not hard real-time deadlines**.

They are part of the fairness mechanism.

### Budgets in the fair scheduler

For ordinary Linux threads, CPU budgets are implicit.

The approximate entitlement is:

\[
\text{CPU share}_i = \frac{w_i}{\sum_j w_j}
\]

For explicit CPU bandwidth limits, Linux provides cgroups:

```text
cpu.max = 20000 100000
```

This limits the cgroup to 20 ms of aggregate CPU time per 100 ms period.

Unlike a nice value, this is an enforced quota.

## 2.2 SCHED_FIFO and SCHED_RR

These are traditional fixed-priority real-time policies.

Linux provides RT priorities 1–99.

`SCHED_FIFO`:

- Highest-priority runnable thread executes.
- Higher-priority threads preempt lower-priority threads.
- Equal-priority threads don't normally preempt one another due to time passage alone.
- Execution continues until blocking, yielding, or preemption.

`SCHED_RR` adds a quantum for rotation among equal-priority threads.

But **the RR quantum is not a CPU budget**.

A single runnable RR thread can still consume 100% of a CPU.

Linux additionally provides aggregate RT bandwidth throttling, commonly configured through:

```text
/proc/sys/kernel/sched_rt_period_us
/proc/sys/kernel/sched_rt_runtime_us
```

Typical defaults allow RT tasks to consume up to 950 ms per 1-second period.

These are aggregate limits, not individual reservations.

## 2.3 SCHED_DEADLINE

Linux SCHED_DEADLINE combines:

- Earliest Deadline First (EDF).
- Constant Bandwidth Server (CBS).

Each thread requests three parameters:

```text
runtime  =  2 ms
deadline =  8 ms
period   = 10 ms
```

Meaning:

> Provide up to 2 ms of CPU execution per 10 ms period, with a scheduling deadline 8 ms after the beginning of each period.

The utilization reservation is:

\[
U = \frac{Q}{P}
\]

For this example:

\[
U = \frac{2}{10} = 0.2
\]

The thread reserves 20% of one CPU.

Linux performs admission control against available scheduling capacity.

When a thread exhausts its runtime, CBS throttles it until replenishment.

Unlike EEVDF, this is an explicit bandwidth reservation.

### Linux summary

Linux offers three substantially different resource models:

| Model | Priority | CPU entitlement |
|---|---|---|
| EEVDF | Virtual deadline | Proportional share |
| FIFO/RR | Fixed RT priority | No individual periodic reservation |
| DEADLINE | Absolute deadline | Explicit runtime/period reservation |

---

# 3. MuQSS: Con Kolivas's deadline-based interactive scheduler

**MuQSS (Multiple Queue Skiplist Scheduler)** is an alternative Linux CPU scheduler developed by Con Kolivas as the successor to BFS (Brain Fuck Scheduler).

Its primary goals are:

- Low scheduling latency.
- Excellent desktop responsiveness.
- Predictable scheduling behavior.
- Simple scheduling decisions.
- Good multicore scalability.

MuQSS is particularly interesting because it combines **fixed time slices, nice-dependent virtual deadlines, and priority-ordered run queues**.

It achieves interactive responsiveness without the complex sleep-history heuristics traditionally associated with desktop scheduling.

MuQSS was originally introduced in 2016. Notably, Kolivas revived the out-of-tree `-ck` patchset in August 2026, targeting Linux 7.2 with additional features including I/O-aware scheduling and accounting kernel-thread work to the originating task. The revived scheduler remains outside mainline Linux. [Linux-Kernel Archive](https://lkml.iu.edu/2608.2/00788.html?utm_source=chatgpt.com)

The following describes the core MuQSS algorithm documented in the original design.

## 3.1 Scheduling model

MuQSS uses:

- One run queue per CPU.
- An eight-level skip list for each run queue.
- Static priority ordering, followed by virtual deadlines.
- A configurable round-robin interval.
- Cross-CPU task selection to improve responsiveness.

The run queues are ordered so that the scheduler can retrieve the best candidate in constant time.

For a single run queue:

| Operation | Complexity |
|---|---|
| Insert thread | O(log N) |
| Select next thread | O(1) |
| Remove thread | Amortized O(1) |

Cross-CPU selection can examine other CPUs' run queues, making that part of scheduling O(P), where P is the number of CPUs.

This is a deliberate tradeoff: MuQSS spends some additional effort finding a good task globally rather than relying exclusively on local run-queue decisions. [Linux-Kernel Archive](https://lkml.iu.edu/1612.1/01866.html?utm_source=chatgpt.com)

## 3.2 Virtual deadlines

MuQSS calculates virtual deadlines using:

\[
D = \text{niffies} + (\text{prio\_ratio} \times \text{rr\_interval})
\]

Where:

- `niffies` is a monotonic, nanosecond-resolution time counter.
- `prio_ratio` depends on the thread's nice level.
- `rr_interval` is the configured round-robin interval.

The priority ratio increases by approximately 10% per nice level.

Higher-priority threads receive earlier virtual deadlines.

For example, suppose:

```text
rr_interval = 6 ms
```

Two threads become runnable at the same time:

```text
Thread A: nice 0
Thread B: nice 5
```

B has a larger priority ratio, so its virtual deadline is later.

A will generally be preferred.

**Unlike Linux EEVDF, MuQSS does not use accumulated virtual runtime and lag as the central mechanism for scheduling eligibility.**

Instead, it assigns a virtual deadline to a slice of CPU time and preserves that deadline until the slice is exhausted.

This is a substantial algorithmic difference despite both schedulers using virtual deadlines.

## 3.3 Time slices and budget accounting

Every ordinary runnable thread receives a time slice corresponding to `rr_interval`.

For example:

```text
Thread A:
    time_slice = 6 ms
    deadline   = 20 ms
```

Suppose A executes for 2 ms and then blocks.

Its state becomes:

```text
Thread A:
    time_slice = 4 ms
    deadline   = 20 ms
```

When A wakes, **it retains both the remaining slice and its original deadline**.

This is a key property of MuQSS.

The scheduler does not simply assign a fresh deadline whenever a thread wakes.

Consequently, an interactive thread that frequently sleeps can retain an earlier deadline than CPU-intensive threads that continuously exhaust their slices.

Consider:

```text
A: interactive
B: CPU-intensive

Time →
A: [run] [sleep........] [run] [sleep........] [run]
B:       [run][run][run]       [run][run]
```

B repeatedly exhausts its time slices and receives new deadlines.

A retains its deadline while sleeping.

When A becomes runnable again, it often has an earlier deadline than B and can preempt it.

This produces interactive responsiveness without requiring an explicit interactive-thread classification heuristic.

### What happens when the slice expires?

When A exhausts its time slice:

1. Its current execution interval ends.
2. Its time slice is replenished.
3. Its virtual deadline is recalculated.
4. It competes again for CPU time.

Crucially, **MuQSS's time slice is a quantum, not a periodic CPU budget**.

The thread does not become ineligible for a fixed replenishment period.

If it remains the best runnable candidate, it can execute again.

This makes MuQSS much closer to EEVDF than to Linux SCHED_DEADLINE or seL4 MCS in terms of resource enforcement.

## 3.4 Priority handling

MuQSS distinguishes between real-time and ordinary scheduling priorities.

For ordinary threads, nice values influence virtual deadlines.

For real-time threads, static priorities take precedence.

Conceptually:

```text
RT priority
    ↓
Scheduling policy / priority class
    ↓
Nice-dependent virtual deadline
    ↓
Next runnable thread
```

The exact ordering is implemented through MuQSS's priority and deadline comparisons.

Unlike a pure EDF scheduler, MuQSS does not simply choose the globally earliest deadline regardless of scheduling class.

## 3.5 SCHED_ISO: an unusual middle ground

One of MuQSS's distinctive features is `SCHED_ISO`, an *isochronous* scheduling policy inherited from BFS.

It was designed for workloads such as audio and multimedia processing that benefit from low latency but should not require unrestricted real-time scheduling privileges.

Conceptually, it provides:

- Scheduling treatment above ordinary workloads.
- No requirement for full real-time scheduling privileges.
- CPU usage monitoring.
- Automatic demotion when the configured CPU usage threshold is exceeded.

This is quite different from a traditional real-time reservation.

Suppose an isochronous thread consumes substantial CPU time continuously.

Rather than allowing it to monopolize the CPU indefinitely at elevated priority, MuQSS can temporarily reduce its scheduling precedence.

Once its measured CPU usage falls sufficiently, it can regain isochronous treatment.

The associated configuration includes an `iso_cpu` threshold.

This creates an interesting distinction:

| Mechanism | What happens when CPU usage is excessive? |
|---|---|
| Linux SCHED_FIFO | Normally continues at RT priority |
| Linux SCHED_RR | Rotates among equal-priority RT threads |
| Linux SCHED_DEADLINE | Throttled until replenishment |
| seL4 MCS | Scheduling context becomes ineligible |
| MuQSS SCHED_ISO | Falls back to ordinary scheduling precedence |

**SCHED_ISO is best understood as CPU-usage-controlled priority, not a strict CPU reservation.**

The thread can continue executing after demotion, but without the elevated scheduling precedence.

## 3.6 SCHED_IDLEPRIO

MuQSS also introduces `SCHED_IDLEPRIO`.

This provides extremely low scheduling precedence for work that should run only when more important tasks do not need the CPU.

Examples include:

- Background compilation.
- Indexing.
- Batch processing.
- Maintenance tasks.

Unlike a strict CPU quota, idle-priority scheduling doesn't enforce a particular percentage.

It simply makes the task yield scheduling precedence to more important work.

## 3.7 MuQSS versus EEVDF

This is probably the most interesting comparison.

| Property | MuQSS | Linux EEVDF |
|---|---|---|
| Primary objective | Interactive responsiveness | Weighted fairness and latency |
| Scheduling metric | Virtual deadline | Eligibility lag + virtual deadline |
| Nice handling | Deadline scaling | Weighted virtual time |
| Time slice | Explicit `rr_interval` | Requested slice, scheduling granularity |
| Sleeping thread | Retains remaining slice and deadline | Lag preservation/decay mechanisms |
| CPU accounting | Slice-based | Continuous service accounting |
| Run queue | Per-CPU skip list | Fair scheduling run-queue structures |
| Cross-CPU selection | Explicit search across run queues | Load balancing and migration |
| Hard CPU budgets | No | No, not in fair class |
| Real-time guarantees | No | No |

The philosophical distinction is important.

**MuQSS uses deadlines to make scheduling latency predictable and to distribute CPU time. EEVDF uses deadlines together with lag to approximate an ideal weighted fair scheduler.**

MuQSS's approach is comparatively direct:

```text
Assign slice
    ↓
Calculate deadline
    ↓
Run earliest eligible task
    ↓
Preserve deadline across sleeps
    ↓
Recalculate when slice expires
```

EEVDF instead continuously reasons about how much CPU service a thread is owed relative to ideal fairness.

Neither provides hard real-time guarantees through these mechanisms.

### MuQSS versus BFS

BFS originally used a single global run queue, which made global scheduling decisions relatively straightforward but introduced scalability problems on larger multicore systems.

MuQSS replaces this with per-CPU run queues and skip lists.

The essential virtual-deadline scheduling idea remains similar.

This preserves the responsiveness-oriented scheduling behavior while reducing global lock contention.

Kolivas's original documentation explicitly describes MuQSS as a scalable evolution of BFS. [Linux-Kernel Archive](https://lkml.iu.edu/hypermail/linux/kernel/1610.3/04905.html?utm_source=chatgpt.com)

---

# 4. seL4: capability-controlled CPU time

seL4 is particularly interesting because its Mixed-Criticality Systems (MCS) extensions make CPU time an explicitly managed kernel resource.

## 4.1 Traditional seL4

The original scheduler uses:

- Fixed priorities, typically 0–255.
- Highest-priority runnable thread wins.
- Round-robin scheduling among equal-priority threads.
- Per-thread timeslices.
- No general periodic CPU reservation.

A high-priority CPU-bound thread can starve lower-priority threads.

## 4.2 seL4 MCS: scheduling contexts

MCS introduces a separate kernel object called a **Scheduling Context (SC)**.

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

A thread requires an eligible scheduling context to execute.

The context defines an upper bound on CPU consumption:

\[
U_{\max}=\frac{\text{budget}}{\text{period}}
\]

The scheduler still selects the highest-priority eligible runnable thread.

But when a high-priority thread exhausts its budget, it becomes ineligible until replenishment.

Unlike MuQSS, replenishing the execution budget is not simply a consequence of exhausting a timeslice.

### Sporadic replenishment

Suppose:

```text
Budget = 3 ms
Period = 10 ms
```

The thread consumes:

```text
t=0:  1 ms
t=3:  2 ms
```

Conceptually, these consumptions create future replenishments:

```text
t=10: +1 ms
t=13: +2 ms
```

The implementation uses refill records, with rules for merging and advancing them.

This prevents a thread from repeatedly consuming more than its configured long-term execution entitlement.

### Scheduling-context donation

One of MCS's most distinctive features is that scheduling contexts are independent of threads.

Consider:

```text
Client A ──RPC──> Server S
Client B ──RPC──> Server S
```

A passive server may have no scheduling context of its own.

When A calls S, the server executes using A's scheduling context.

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
S executes using A's budget
    |
    v
CPU time charged to A
```

When S replies, the scheduling context returns.

This provides a useful form of temporal isolation: one client's requests cannot indefinitely consume CPU time allocated to another client merely by using the same server.

Scheduling-context donation does not automatically imply priority donation.

### What seL4 doesn't guarantee

A budget is an upper bound, not a guaranteed minimum allocation.

Suppose:

```text
A: priority 200, budget 8/10 ms
B: priority 100, budget 5/10 ms
```

The combined requested utilization is 130%.

The kernel cannot guarantee both budgets will be delivered.

Unlike Linux SCHED_DEADLINE, seL4 MCS does not generally perform admission control for these allocations.

Schedulability remains the system designer's responsibility.

### seL4 versus MuQSS

| Property | MuQSS | seL4 MCS |
|---|---|---|
| Primary scheduling order | Priority, then virtual deadline | Fixed priority |
| Execution allowance | Timeslice | Budget/period |
| On exhaustion | Renew slice and deadline | Wait for replenishment |
| CPU isolation | No strict per-thread budget | Explicit upper bound |
| Scheduling resource transferable | No | Yes, scheduling contexts |
| Admission control | No | No general admission control |
| Main objective | Interactive responsiveness | Temporal isolation |

The most important difference is that **MuQSS manages the order in which CPU time is distributed, while seL4 MCS additionally manages the authority to consume CPU time**.

---

# 5. Windows: dynamic priorities and quantums

Windows uses a priority-driven preemptive scheduler.

Its scheduling priorities range from 0 to 31.

| Priority | Meaning |
|---|---|
| 0 | Special system priority |
| 1–15 | Variable-priority range |
| 16–31 | Real-time range |

The scheduler uses priority boosts to improve responsiveness.

For example:

```text
CPU-bound thread:
    Base priority = 8

Interactive thread:
    Base priority = 8
    I/O completion → temporary boost to 12
```

The interactive thread preempts the CPU-bound thread.

Its priority subsequently decays toward the base level.

## Quantums

Threads receive execution quantums.

When a quantum expires, another thread at the same priority may run.

As with MuQSS, a quantum is not a periodic CPU reservation.

A thread can continue consuming CPU if no equally or more highly prioritized thread needs execution.

## Budget enforcement

Traditional Windows scheduling does not provide an interface equivalent to Linux's `(runtime, deadline, period)`.

However, Windows Job Objects can enforce CPU rate limits on groups of processes.

These limits are separate from thread priorities and quantum scheduling.

### Windows versus MuQSS

Both target interactive responsiveness, but their mechanisms differ.

Windows relies substantially on dynamic priority adjustments, while MuQSS relies on virtual deadlines and preservation of unused timeslices across sleeps.

MuQSS's original design explicitly avoids complex interactivity heuristics.

---

# 6. macOS: QoS and time constraints

Darwin combines priority scheduling with Quality of Service (QoS) classifications.

| QoS | Intended use |
|---|---|
| User Interactive | UI interaction |
| User Initiated | User-requested work |
| Default | Ordinary work |
| Utility | Longer-running work |
| Background | Maintenance and indexing |

QoS influences scheduling priority and other resource-management policies.

Darwin also supports real-time scheduling constraints through Mach:

```c
thread_time_constraint_policy_data_t
```

Its fields include:

```c
period
computation
constraint
preemptible
```

These describe a thread's expected execution requirements.

However, this interface should not be interpreted as providing the same reservation guarantees as Linux SCHED_DEADLINE.

macOS also propagates scheduling importance through some synchronization and work-dependency mechanisms.

### macOS versus MuQSS

macOS relies on explicit workload classification, dynamic priorities, and system-level policies.

MuQSS instead aims to make ordinary interactive applications responsive without requiring special scheduling classifications.

Both favor latency-sensitive workloads, but MuQSS attempts to achieve that through a comparatively uniform scheduling algorithm.

---

# 7. FreeBSD: ULE

FreeBSD's default scheduler, ULE, combines:

- Dynamic priorities.
- Interactivity heuristics.
- Per-CPU run queues.
- CPU affinity.
- Load balancing.

ULE distinguishes interactive and CPU-intensive threads partly through recent execution and sleep behavior.

```text
Thread A:
    Frequently sleeps
    → Interactive

Thread B:
    Continuously consumes CPU
    → CPU-intensive
```

Interactive threads can receive favorable scheduling treatment.

ULE also tries to preserve cache locality by avoiding unnecessary migrations.

FreeBSD supports real-time scheduling policies such as `SCHED_FIFO` and `SCHED_RR`.

However, ULE doesn't normally associate explicit periodic CPU budgets with individual threads.

### FreeBSD versus MuQSS

Both support per-CPU run queues and interactive scheduling.

The difference is in how interactivity is recognized.

ULE uses dynamic priority calculations influenced by observed behavior.

MuQSS relies primarily on virtual deadlines and preserving unused timeslices.

---

# 8. QNX: fixed priorities and adaptive partitioning

QNX Neutrino supports traditional real-time scheduling alongside more sophisticated CPU partitioning.

## 8.1 Traditional QNX scheduling

QNX supports:

- `SCHED_FIFO`
- `SCHED_RR`
- `SCHED_SPORADIC`

FIFO and RR behave similarly to their counterparts in other real-time operating systems.

Sporadic scheduling is more interesting.

### Sporadic scheduling

A thread has:

```text
High priority: 20
Low priority:   5

Budget: 2 ms
Period: 10 ms
```

Initially, it executes at priority 20.

When its high-priority budget is exhausted, it drops to priority 5.

It can continue executing at the lower priority.

When the budget is replenished, it can regain high-priority execution.

This resembles MuQSS's `SCHED_ISO` in one respect: **excessive CPU consumption results in reduced scheduling precedence rather than complete ineligibility**.

However, their accounting mechanisms differ.

QNX sporadic scheduling uses an explicit replenishment period and high-priority execution budget.

MuQSS SCHED_ISO monitors aggregate isochronous CPU usage and temporarily demotes isochronous tasks when the configured threshold is exceeded.

## 8.2 Adaptive Partitioning Scheduler (APS)

QNX APS allocates CPU bandwidth to partitions.

For example:

```text
Partition A: Control software  50%
Partition B: Device drivers    30%
Partition C: Diagnostics       20%
```

Each partition contains threads with their own priorities.

```text
Global scheduler
    |
    +-- Partition A: 50%
    |       +-- Thread A1: priority 30
    |       +-- Thread A2: priority 20
    |
    +-- Partition B: 30%
    |       +-- Thread B1: priority 40
    |
    +-- Partition C: 20%
            +-- Thread C1: priority 15
```

The scheduler combines thread priorities with partition bandwidth accounting.

If diagnostics is idle, other partitions can borrow unused CPU capacity.

When diagnostics needs CPU time again, the scheduler can constrain competing partitions to preserve its allocation.

APS accounts for CPU usage over an averaging window rather than merely resetting budgets at fixed period boundaries.

This makes it a form of work-conserving resource partitioning.

### QNX versus MuQSS

| Property | MuQSS | QNX APS |
|---|---|---|
| Scheduling entity | Thread | Thread within partition |
| Priority | Static class + virtual deadline | Fixed thread priority |
| Budget | Timeslice | Partition bandwidth |
| Unused CPU | Available to other threads | Borrowable by partitions |
| Isolation | No strict CPU partitioning | Partition bandwidth enforcement |
| Main objective | Responsiveness | Predictable subsystem allocation |

---

# 9. Other interesting systems

## Zephyr

Zephyr is a configurable embedded RTOS.

It supports:

- Cooperative threads.
- Fixed-priority preemptive threads.
- Optional round-robin timeslicing.
- Optional deadline-related scheduling features.

Its ordinary scheduler does not provide general periodic CPU reservations.

Applications are typically responsible for ensuring that their task sets are schedulable.

## RTEMS

RTEMS supports multiple scheduler implementations, including fixed-priority and EDF variants.

Rather than providing one universal scheduling policy, RTEMS allows an embedded system to select a scheduler appropriate for its workload.

## Xen

Xen schedules virtual CPUs rather than application threads.

Its RTDS scheduler uses budget and period parameters.

For example:

```text
VM A / vCPU 0:
    Budget = 4 ms
    Period = 10 ms

VM B / vCPU 0:
    Budget = 6 ms
    Period = 10 ms
```

This creates another scheduling level:

```text
Hypervisor scheduler
        ↓
Guest OS scheduler
        ↓
Application thread
```

Even if the guest OS schedules an application correctly, its deadlines can be missed if the hypervisor doesn't provide sufficient execution capacity.

This motivates hierarchical scheduling and compositional schedulability analysis.

---

# 10. Four fundamentally different meanings of CPU budget

The word *budget* is used for several mechanisms that should be distinguished.

## 10.1 Quantum

A quantum is an execution interval after which the scheduler may reconsider its choice.

```text
Quantum = 5 ms
```

After 5 ms, the thread may be moved behind other runnable threads.

But it can immediately execute again.

A quantum doesn't limit total CPU consumption.

Examples:

- MuQSS `rr_interval`.
- Windows timeslices.
- Linux `SCHED_RR`.
- Traditional seL4 timeslices.

## 10.2 Quota

A quota limits CPU consumption over some interval.

```text
Quota  = 20 ms
Period = 100 ms
```

Once the quota is exhausted, execution is restricted until replenishment.

Examples:

- Linux cgroup CPU quotas.
- seL4 MCS scheduling contexts.

A quota is an upper bound, not necessarily a guaranteed allocation.

## 10.3 Reservation

A reservation promises CPU capacity under specified assumptions.

```text
Runtime  = 2 ms
Period   = 10 ms
Deadline = 8 ms
```

The scheduler must account for competing reservations.

This typically requires admission control or offline schedulability analysis.

Example: Linux `SCHED_DEADLINE`.

## 10.4 Share

A share determines how CPU capacity is distributed under contention.

```text
A: weight 100
B: weight 200
```

When both are runnable:

```text
A: approximately 33%
B: approximately 67%
```

If B sleeps, A can use the entire CPU.

Examples include Linux EEVDF and weighted CPU resource controls.

### Comparison

| Property | Quantum | Quota | Reservation | Share |
|---|---|---|---|---|
| Limits continuous execution | Yes | Sometimes | Sometimes | Indirectly |
| Limits total CPU usage | No | Yes | Usually enforced | No |
| Guarantees CPU allocation | No | No | Conditionally | Relative fairness |
| Requires replenishment | No | Yes | Yes | No |
| Work-conserving by default | Yes | Not necessarily | Depends | Yes |
| Temporal isolation | No | Yes | Yes | Limited |

MuQSS's ordinary scheduling model is primarily quantum-based, even though its time slices are sometimes described as quotas in its documentation.

---

# 11. Priority inversion and budget inheritance

Once priorities and budgets are separated, an interesting problem appears.

Consider:

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

A calls S and waits for a reply.

But B has higher priority than S, so B can prevent S from executing.

This is priority inversion.

Traditional systems address it using priority inheritance or priority ceiling protocols.

With explicit CPU budgets, however, two independent questions arise:

1. At what priority should S execute?
2. Whose CPU budget should S consume?

There are four conceptual combinations:

| Priority | Budget | Interpretation |
|---|---|---|
| Server | Server | Independent server |
| Client | Server | Priority inheritance |
| Server | Client | Budget donation |
| Client | Client | Priority and budget inheritance |

### Priority inheritance

S executes at A's priority but consumes its own budget.

This solves priority inversion, but a client can still exhaust a shared server's CPU allocation.

### Budget donation

S consumes A's budget but executes at its own priority.

This isolates CPU accounting, but doesn't necessarily solve priority inversion.

### Both

S executes at A's effective priority using A's budget.

This can be useful for synchronous RPC chains:

```text
A → S1 → S2 → S3
```

CPU consumption throughout the chain can be charged to A.

seL4 MCS explicitly supports scheduling-context donation, although it does not automatically imply unrestricted priority inheritance.

MuQSS doesn't provide an equivalent general scheduling-context donation mechanism.

Its virtual deadlines are properties of individual tasks rather than transferable execution authorities.

---

# 12. Fixed-priority scheduling versus EDF

Another important distinction is between static priorities and dynamic deadline ordering.

## Rate Monotonic Scheduling (RMS)

RMS assigns fixed priorities based on task periods.

Shorter-period tasks receive higher priorities.

Consider:

| Thread | Execution | Period | Priority |
|---|---:|---:|---|
| A | 1 ms | 4 ms | High |
| B | 2 ms | 5 ms | Medium |
| C | 2 ms | 10 ms | Low |

Their utilization is:

\[
U=\frac14+\frac25+\frac2{10}=0.85
\]

For three independent periodic tasks with deadlines equal to periods, the classical sufficient RMS utilization bound is:

\[
U\leq3(2^{1/3}-1)\approx0.7798
\]

This task set exceeds the sufficient bound, but that doesn't necessarily mean it is unschedulable.

More precise response-time analysis may establish schedulability.

## Earliest Deadline First

EDF dynamically selects the job with the earliest absolute deadline.

For independent, preemptible periodic tasks on one processor, with deadlines equal to periods and negligible overhead, EDF can schedule task sets satisfying:

\[
U\leq1
\]

However, EDF and fixed-priority scheduling behave differently under overload.

Fixed-priority scheduling tends to preserve execution for higher-priority tasks at the expense of lower-priority ones.

EDF can distribute deadline misses more broadly when the system becomes overloaded.

This is one reason fixed-priority scheduling remains attractive for safety-critical systems.

### Where MuQSS fits

MuQSS uses earliest-effective-virtual-deadline scheduling for ordinary tasks, but it isn't a conventional real-time EDF scheduler.

Its deadlines:

- Are derived from nice levels and time slices.
- Aren't application completion deadlines.
- Don't represent guaranteed execution deadlines.
- Are recalculated when slices expire.
- Are preserved across sleeps.

Consequently, the classical EDF schedulability results don't apply directly to MuQSS.

---

# 13. Overall comparison

This table brings the different scheduling models together.

| OS / Scheduler | Priority model | Time budget | Exhaustion behavior | Primary objective |
|---|---|---|---|---|
| **Linux EEVDF** | Virtual deadlines + eligibility | Weighted service share | Accumulate service debt | Fairness and latency |
| **MuQSS** | Priority + virtual deadline | Fixed timeslice | Renew slice and deadline | Interactive responsiveness |
| **MuQSS SCHED_ISO** | Elevated pseudo-RT priority | CPU usage threshold | Demote to normal precedence | Low-latency multimedia |
| Linux FIFO | Fixed priority | None by default | N/A | RT responsiveness |
| Linux RR | Fixed priority | Quantum | Rotate among peers | RT time sharing |
| Linux DEADLINE | EDF | Runtime/period | Throttle/replenish | RT reservations |
| Windows | Dynamic priority | Quantum | Reschedule | Responsiveness |
| macOS | QoS + priority | Quantum / RT constraints | Policy-dependent | Latency and energy |
| FreeBSD ULE | Dynamic priority | Timeslice | Reschedule | Interactive throughput |
| seL4 traditional | Fixed priority | Timeslice | Rotate among peers | Simple RT scheduling |
| seL4 MCS | Fixed priority | Sporadic budget/period | Ineligible until refill | Temporal isolation |
| QNX FIFO/RR | Fixed priority | None / quantum | Continue / rotate | Deterministic RT |
| QNX sporadic | High/low fixed priorities | Sporadic high-priority budget | Drop priority | RT responsiveness |
| QNX APS | Priority + partition | Partition bandwidth | Partition constrained | CPU isolation |
| Zephyr | Fixed priority | Optional timeslice | Rotate among peers | Embedded RT |
| RTEMS EDF | Dynamic deadline | Task execution requirements | Policy-dependent | Embedded RT |
| Xen RTDS | Deadline-based | vCPU budget/period | Replenishment | VM-level RT |

---

# 14. Architectural comparison

Four approaches stand out.

### Linux EEVDF: scheduling as proportional fairness

EEVDF treats CPU time as a continuously distributed resource.

The scheduler tracks which threads are owed service and uses virtual deadlines to decide which should execute.

Applications don't need to declare their execution requirements.

This is well suited to general-purpose systems but doesn't provide hard real-time guarantees.

### MuQSS: scheduling as latency management

MuQSS uses a comparatively simple mechanism:

```text
Assign timeslice
       ↓
Calculate virtual deadline
       ↓
Select earliest effective deadline
       ↓
Preserve deadline across sleeps
       ↓
Recalculate on slice exhaustion
```

Its distinctive insight is that **preserving a sleeping thread's unused slice and deadline naturally favors interactive workloads**.

A thread that frequently blocks on input doesn't need to be classified as interactive through elaborate heuristics.

It simply tends to retain an earlier deadline than threads that continuously consume their slices.

The scheduler's primary concern is distributing CPU opportunities with low latency, not enforcing resource reservations.

### seL4 MCS: scheduling as resource authority

seL4 MCS treats CPU execution capacity as an explicitly managed resource.

A scheduling context represents permission to consume CPU time within configured bounds.

This makes it possible to:

- Enforce CPU consumption limits.
- Delegate execution capacity.
- Donate CPU budgets across IPC.
- Isolate clients sharing userspace servers.

It is particularly well suited to microkernel architectures where CPU resource management should follow explicit delegation principles.

### QNX APS: scheduling as resource partitioning

QNX APS reserves CPU capacity for groups of threads rather than requiring every thread to have an individual reservation.

It combines ordinary priority scheduling with partition-level bandwidth accounting.

Unused capacity can be borrowed, but competing partitions can be constrained when reservations need to be honored.

This is useful for systems containing several subsystems with different criticality requirements.

---

## 15. Three questions that reveal a scheduler's semantics

When examining an unfamiliar scheduler, I would start with these questions.

**First: What happens when a thread exhausts its quantum or budget?**

Does it rotate, receive a new deadline, become ineligible, drop priority, or accumulate scheduling debt?

For example:

- MuQSS: renew the slice and virtual deadline.
- MuQSS SCHED_ISO: demote after excessive CPU usage.
- seL4 MCS: wait for budget replenishment.
- QNX sporadic: drop to a lower priority.
- Linux SCHED_DEADLINE: throttle until replenishment.

These produce fundamentally different scheduling guarantees.

**Second: Does the scheduler enforce upper bounds, guarantee minimum service, or merely distribute CPU fairly?**

This distinguishes CPU isolation from CPU reservations and fair-share scheduling.

MuQSS and EEVDF primarily distribute CPU opportunities.

seL4 MCS enforces execution limits.

Linux SCHED_DEADLINE combines bandwidth enforcement with admission-controlled reservations.

**Third: Who pays for CPU time spent servicing another thread?**

This matters particularly in microkernels.

If a client invokes a filesystem server or device driver, is the CPU time charged to the client or the server?

The answer determines whether shared services can provide temporal isolation between clients.

---

## Further reading

The most useful primary sources for understanding these mechanisms are:

- **[Con Kolivas — MuQSS documentation](https://www.mail-archive.com/linux-kernel@vger.kernel.org/msg1260699.html)** — complete original description of virtual deadlines, time slices, run queues, SCHED_ISO, and SCHED_IDLEPRIO.
- **[MuQSS's 2026 revival](https://lkml.iu.edu/2608.2/00788.html)** — new features and the return of the `-ck` patchset.
- **[Linux EEVDF documentation](https://kernel.org/doc/html/latest/scheduler/sched-eevdf.html)** — virtual deadlines, lag, and eligibility.
- **[Linux SCHED_DEADLINE documentation](https://www.kernel.org/doc/html/latest/scheduler/sched-deadline.html)** — EDF, CBS, replenishment, and admission control.
- **[seL4 MCS tutorial](https://docs.sel4.systems/Tutorials/mcs.html)** — scheduling contexts and budget donation.
- **[QNX Adaptive Partitioning documentation](https://www.qnx.com/developers/docs/6.4.0/adaptive_partitioning_en/user_guide/faq_ap.html)** — partition bandwidth and guarantees.

### Final perspective

The most revealing comparison is between **MuQSS, Linux EEVDF, and seL4 MCS**.

All three account for CPU execution, but they use that accounting for different purposes.

MuQSS uses consumed timeslices to advance virtual deadlines, producing responsive scheduling without complex interactivity heuristics.

EEVDF uses accumulated service and eligibility lag to approximate weighted fairness while allowing latency-sensitive scheduling.

seL4 MCS uses explicit execution budgets to enforce temporal resource boundaries independently of priority.

These illustrate three distinct interpretations of CPU time: **an opportunity to schedule, a share of a resource, and an authority to execute**.
