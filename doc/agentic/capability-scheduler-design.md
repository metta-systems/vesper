Preparation work: [capabilities-time](capabilities-time.md) and [capabilities-scheduling](capabilities-scheduling.md).

# Designing a policy-free scheduling subsystem for a microkernel

I think your proposed separation between **`Time`** and **`Scheduler`** is an excellent starting point, and I'd take it somewhat further than seL4 MCS.

The central architectural idea I'd explore is:

> **The kernel controls who is permitted to execute, for how long, and on which CPU. Userspace controls which permitted thread should execute next and when its execution allowance should be renewed.**

The challenge is deciding how much mechanism the kernel needs to make this efficient without accidentally hardcoding one particular scheduling policy.

My preferred design would have:

1. **`Time`** — a transferable, accounted CPU-time resource.
2. **`Scheduler`** — authority to select threads and grant them execution intervals.
3. **`Thread`** — execution state, independent of both.
4. **A minimal kernel dispatcher** — enforces execution limits and hands control back to userspace.
5. **Scheduling events** — asynchronous notifications of blocking, waking, exhaustion, and other relevant state changes.

I would initially avoid kernel-level priorities, periodic replenishment, virtual deadlines, fairness accounting, and scheduling classes.

Those are policies, and exposing them through the core kernel API would unnecessarily constrain what userspace can implement.

There are, however, a few subtle exceptions.

In particular, **CPU budget enforcement, scheduler execution guarantees, and IPC budget propagation can't be delegated entirely to userspace** without undermining performance or isolation.

Let's build the design from first principles.

---

# 1. What should the kernel actually guarantee?

I'd start with a fairly strict list of kernel responsibilities.

### Mechanisms the kernel must provide

| Mechanism | Why it belongs in the kernel |
|---|---|
| CPU execution accounting | Only the kernel can reliably account for execution |
| Execution preemption | Userspace cannot preempt an uncooperative running thread |
| Capability enforcement | Prevents unauthorized scheduling or budget allocation |
| Thread state transitions | Blocking, waking, and termination involve kernel state |
| Scheduler activation | Something must run when a thread blocks or exhausts its allowance |
| Timer programming | Needed to preempt threads at precise budget boundaries |
| CPU ownership | Prevents conflicting scheduling decisions |
| IPC budget transfer | Must be atomic with relevant execution-state transitions |
| Multicore dispatch | Needs synchronization with CPU execution state |

### Policies I'd keep outside the kernel

| Policy | Userspace implementation |
|---|---|
| Fixed-priority scheduling | Priority queues |
| Round-robin | Queue rotation and timeslices |
| MuQSS-style scheduling | Virtual deadlines and slice accounting |
| CFS/EEVDF | Virtual runtime, lag, virtual deadlines |
| EDF | Absolute deadline queues |
| Rate-monotonic scheduling | Static priorities derived from periods |
| Sporadic servers | Budget accounting and replenishment |
| CPU shares | Hierarchical weighted allocation |
| Priority inheritance | Dependency tracking and priority propagation |
| Work stealing | Scheduler-to-scheduler migration |
| CPU affinity preferences | Scheduler policy |
| Load balancing | Scheduler policy |

Notice that I'm distinguishing **enforcing a CPU execution limit** from **deciding how CPU time is replenished**.

This distinction is important.

A kernel may need to enforce a limit of 2 ms, but it doesn't necessarily need to know whether those 2 ms represent:

- A round-robin quantum.
- A sporadic-server budget.
- An EDF reservation.
- An interactive MuQSS slice.
- A hierarchical CPU allocation.
- A temporary budget loan from another thread.

These should ideally look identical to the kernel.

---

# 2. The three main objects

Here's the object model I'd explore.

```text
                  ┌─────────────────┐
                  │    Scheduler    │
                  │                 │
                  │ Dispatch policy │
                  │ Thread control  │
                  └────────┬────────┘
                           │
                     dispatch()
                           │
                           ▼
                  ┌─────────────────┐
                  │     Thread      │
                  │                 │
                  │ Registers       │
                  │ Execution state │
                  └────────┬────────┘
                           │
                     executes with
                           │
                           ▼
                  ┌─────────────────┐
                  │      Time       │
                  │                 │
                  │ Remaining       │
                  │ Consumed        │
                  │ Accounting      │
                  └─────────────────┘
```

The three objects have independent lifetimes.

A thread doesn't intrinsically own its CPU budget.

A budget doesn't intrinsically belong to a particular thread.

And a scheduler doesn't necessarily own the CPU time that it schedules.

That last distinction is especially important for hierarchical scheduling.

## 2.1 `Thread`: execution state

A thread contains only the state needed to execute it.

Conceptually:

```rust
struct Thread {
    registers: Registers,
    address_space: AddressSpace,
    ipc_state: IpcState,
    exception_handler: Option<Endpoint>,

    state: ThreadState,
    scheduler: SchedulerId, // the important bit
}
```

No permanent priority.

No permanent quantum.

No virtual runtime.

No CPU budget.

The kernel needs to know which scheduler is responsible for the thread, but that relationship should be changeable through an authorized operation.

A thread might have states such as:

```rust
enum ThreadState {
    Runnable,
    Running,
    Blocked,
    Suspended,
}
```

I'd avoid storing `BudgetExhausted` as a thread state.

Budget exhaustion should be independent of whether the thread is logically runnable.

A runnable thread can simply lack permission to execute until its scheduler provides more time.

That distinction becomes useful for EDF, sporadic servers, and hierarchical scheduling.

## 2.2 `Time`: execution authority

I'd define `Time` as a resource representing an amount of CPU execution time that may be consumed.

A very simple starting point:

```rust
struct Time {
    remaining: Duration,
    consumed: Duration,
}
```

But this raises a fundamental question:

**Where does the time come from?**

If arbitrary userspace can increase `remaining`, then the object isn't actually providing resource isolation.

Consequently, `Time` needs either an accounting hierarchy or a separate replenishment authority.

We'll return to this shortly.

## 2.3 `Scheduler`: scheduling authority

A `Scheduler` object establishes authority over a collection of threads.

Conceptually:

```rust
struct Scheduler {
    threads: Set<ThreadId>,
    event_queue: Queue<SchedulingEvent>,
    cpu_set: CpuSet,
}
```

The scheduler's userspace implementation can maintain whatever data structures it wants.

For example:

```rust
struct MuqssScheduler {
    queues: [SkipList; CPU_COUNT],
}

struct EdfScheduler {
    deadlines: BinaryHeap<ThreadId>,
}

struct RoundRobinScheduler {
    runnable: VecDeque<ThreadId>,
}
```

The kernel doesn't need to know which algorithm is being used.

The `Scheduler` kernel object should primarily provide authorization, state tracking, and event delivery.

---

# 3. The most important primitive: `dispatch`

Rather than exposing kernel operations such as:

```text
set_priority()
set_deadline()
set_timeslice()
set_policy()
```

I'd expose a general operation:

```rust
Scheduler::dispatch(
    thread: ThreadCap,
    time: TimeCap,
    duration: Duration,
)
```

Semantically:

> Execute this thread, using the specified time resource, for at most the specified duration.

For example:

```rust
scheduler.dispatch(
    thread_a,
    time_a,
    Duration::from_millis(2),
);
```

The kernel verifies that:

1. The caller has scheduling authority over `thread_a`.
2. The time resource is valid and spendable.
3. The requested duration doesn't exceed the available allowance.
4. The thread is runnable.
5. The requested CPU is authorized.
6. The thread isn't already executing elsewhere.

If valid, it installs the execution state.

Conceptually:

```text
CPU 0
  │
  ▼
Scheduler
  │
  │ dispatch(A, TimeA, 2 ms)
  ▼
Thread A executes
  │
  ├── blocks after 0.8 ms
  │       → return control
  │       → charge 0.8 ms
  │
  └── exhausts 2 ms
          → kernel preempts A
          → return control
          → charge 2 ms
```

The remaining time stays in the `Time` object.

The scheduler can then decide what to do.

### Why this is powerful

This one primitive is sufficient to express many scheduling policies.

**Round-robin:**

```rust
loop {
    let thread = queue.pop_front();

    dispatch(thread, thread.time, QUANTUM);

    queue.push_back(thread);
}
```

**Fixed priority:**

```rust
loop {
    let thread = highest_priority_runnable();

    dispatch(thread, thread.time, QUANTUM);
}
```

**EDF:**

```rust
loop {
    let thread = earliest_deadline_runnable();

    dispatch(thread, thread.time, quantum_until_next_event());
}
```

**MuQSS:**

```rust
loop {
    let thread = earliest_virtual_deadline();

    dispatch(thread, thread.time, thread.remaining_slice);

    update_virtual_deadline(thread);
}
```

**Weighted fair scheduling:**

```rust
loop {
    let thread = select_eligible_thread();

    let slice = calculate_fair_slice(thread);

    dispatch(thread, thread.time, slice);

    update_service_accounting(thread);
}
```

In all cases, the kernel sees the same operation.

### One necessary refinement: dispatch must support interruption

The examples above are deliberately simplified.

A real `dispatch()` cannot simply run a thread for 2 ms and then return.

Suppose a higher-priority thread becomes runnable after 100 μs.

The scheduler needs an opportunity to replace the currently executing thread.

Therefore, I'd make the execution interval terminate on:

- Budget exhaustion.
- Thread blocking.
- Explicit yielding.
- A scheduler-requested preemption.
- Selected scheduler-relevant events.
- Thread termination.

That leads to the next important component.

---

# 4. Scheduler activations: how userspace regains control

I think this is the most important part of the interface after `dispatch()`.

A scheduler needs to know when a decision is necessary.

It also needs to preempt a running thread when something important happens.

I'd introduce a small set of scheduling events:

```rust
enum SchedulingEvent {
    Runnable(ThreadId),
    Blocked(ThreadId),
    Yielded(ThreadId, Duration),
    Preempted(ThreadId, Duration),
    BudgetExhausted(ThreadId),
    Exited(ThreadId),
}
```

These are illustrative; they needn't all be independent event types in the actual ABI.

The kernel would deliver events through a scheduler-specific notification channel.

### Example: a higher-priority thread wakes

Suppose:

```text
CPU 0:
    A running (priority 10)

Scheduler:
    B blocked (priority 20)
```

B receives an IPC message and becomes runnable.

```text
                    IPC arrives
                         │
                         ▼
                  Kernel wakes B
                         │
                         ▼
                  Notify Scheduler
                         │
                         ▼
                   Scheduler runs
                         │
                 Compare priorities
                         │
                         ▼
                  Preempt A, run B
```

The kernel doesn't need to understand priorities.

It only needs to notify the scheduler that a thread's state has changed.

The scheduler decides whether preemption is necessary.

## 4.1 The scheduler must be able to preempt immediately

A naive implementation would only invoke the scheduler when the current thread's quantum expires.

That's insufficient for responsive scheduling.

If A has 10 ms remaining but B is a latency-critical thread, B might need to run immediately.

I'd support something like:

```rust
Scheduler::preempt(cpu);
```

Or, preferably, combine preemption and dispatch:

```rust
Scheduler::replace(cpu, thread_b, time_b, duration);
```

This avoids a separate preemption followed by a second dispatch.

Internally, the operation atomically:

1. Stops accounting execution to A.
2. Records A's remaining allowance.
3. Accounts for the context-switch boundary.
4. Installs B's execution context.
5. Arms the next execution timer.

On another core, it may require an IPI.

### Why I prefer atomic replacement

A two-step interface:

```text
preempt(A)
dispatch(B)
```

introduces an interval in which no useful work is selected.

More importantly, it creates race conditions between the operations.

An atomic replacement allows the kernel to validate the expected execution state and commit the scheduling decision as one transaction.

This is closely related to the approach used by Google's **ghOSt**, which delegates scheduling policy to userspace and lets agents commit scheduling decisions through a transactional kernel interface. Its implementations demonstrate that userspace schedulers can support EDF, CFS-like scheduling, centralized FIFO, and other policies with a common mechanism. [Google Research](https://research.google/pubs/ghost-fast-and-flexible-user-space-delegation-of-linux-scheduling/?utm_source=chatgpt.com)

## 4.2 Avoiding stale scheduling decisions

Consider:

```text
Scheduler reads:
    B is runnable

Before dispatch:
    B blocks on another CPU
```

Or:

```text
Scheduler decides:
    Replace A with B

Meanwhile:
    A blocks on an IPC endpoint
```

I'd attach a generation counter to scheduling state.

For example:

```rust
struct DispatchRequest {
    cpu: CpuId,
    expected_generation: u64,
    thread: ThreadCap,
    time: TimeCap,
    duration: Duration,
}
```

The kernel accepts the request only if the relevant state still matches.

If not, it rejects the operation and returns current state.

This permits optimistic scheduling decisions without holding kernel locks while a userspace scheduler performs potentially complicated computations.

---

# 5. The difficult question: who can create or replenish `Time`?

This is where I'd consider two alternative architectures.

They have substantially different properties.

## Design A: `Time` is a consumable execution allowance

In this design:

```rust
struct Time {
    remaining: Duration,
    consumed: Duration,
}
```

A scheduler can grant execution only while the object has remaining time.

Time must be obtained from an authority that can allocate it.

For example:

```text
Root CPU authority
        │
        ▼
Root Scheduler
        │
        ├── Time A: 2 ms
        ├── Time B: 5 ms
        └── Time C: 3 ms
```

The root scheduler can replenish these objects according to any algorithm.

This permits ordinary round-robin scheduling, EDF, sporadic servers, and so on.

However, there's an important problem.

**If the scheduler replenishes time freely, the budgets are not enforced resource limits against that scheduler.**

A scheduler could repeatedly grant 2 ms to A, effectively allowing A to consume unlimited CPU time.

That might be fine for the root scheduler, but not for an untrusted child scheduler.

The solution is hierarchical CPU-time authority.

## 5.1 Hierarchical budget allocation

Imagine:

```text
                      CPU 0
                        │
                        ▼
                 Root Scheduler
                        │
               100% CPU authority
                        │
          ┌─────────────┴─────────────┐
          │                           │
          ▼                           ▼
     Scheduler A                  Scheduler B
     Budget: 30%                 Budget: 70%
          │                           │
     ┌────┴────┐                 ┌────┴────┐
     ▼         ▼                 ▼         ▼
   Thread 1  Thread 2          Thread 3  Thread 4
```

Scheduler A can implement MuQSS.

Scheduler B can implement EDF.

The root scheduler doesn't need to know anything about either policy.

But the kernel must guarantee that A cannot use more CPU time than its parent authorizes.

A possible object model:

```rust
struct Time {
    balance: Duration,
    parent: Option<TimeId>,
}
```

Time can be transferred or subdivided.

For example:

```rust
root_time.transfer(child_time, 2.ms());
```

The crucial invariant is:

\[
\sum \text{outstanding delegated time}
+
\text{locally available time}
\leq
\text{authorized capacity}
\]

This is a conservation rule for *already funded* execution time, not an attempt to represent all future CPU capacity as a finite quantity.

There is also a potential complication: if a parent scheduler allocates its entire current allowance, the parent may not have any CPU time left to execute itself.

That leads to scheduler execution guarantees, which we'll examine shortly.

### Advantages

- Natural capability-based resource delegation.
- No arbitrary CPU usage creation by untrusted schedulers.
- Strong foundation for hierarchical scheduling.
- Easy to reason about accounting ownership.

### Disadvantages

- Requires mechanisms for transferring and reclaiming time.
- Requires decisions about what happens to unspent time.
- Adds complexity to the critical scheduling path.
- Reservation semantics still need to be implemented somewhere.

Most importantly, time conservation alone doesn't specify *when* an allocation becomes available.

Giving a child 20 ms doesn't say whether it may consume those 20 ms immediately or only over the next 100 ms.

That distinction matters for temporal isolation.

---

# 6. Design B: `Time` is an accounting object, while scheduling authority controls execution

This is a different approach, and I think it's worth considering seriously.

Instead of treating `Time` primarily as a resource that must be consumed and replenished, treat it as an **execution accounting context**.

```rust
struct Time {
    consumed_total: Duration,
    execution_limit: Option<Duration>,
}
```

A scheduler dispatches a thread with:

```rust
dispatch(thread, time, duration);
```

The kernel charges execution against `time` and enforces `duration`.

But the scheduler is responsible for deciding whether that thread should execute again.

In this architecture, the ability to dispatch a thread is itself the relevant authority.

An untrusted scheduler is constrained because its parent controls how much execution time the scheduler's domain receives.

Consider:

```text
Root Scheduler
      │
      │ grants execution to domain A
      ▼
Scheduler A
      │
      ├── Thread A1
      ├── Thread A2
      └── Thread A3
```

The root grants A an execution interval of 3 ms.

During that interval, Scheduler A can allocate CPU time however it wants.

It cannot exceed its parent's 3 ms allowance.

The kernel charges execution against both the current thread's accounting object and its enclosing domain's budget.

### Why this is attractive

The kernel doesn't need to implement periodic replenishment.

The root scheduler can implement an EDF server, sporadic server, or another policy.

The child scheduler can independently implement round-robin, MuQSS, or something else.

The kernel merely enforces the maximum execution allowance at each active scheduling level.

Conceptually:

```text
CPU execution
      │
      ├── Charge Thread's Time
      │
      ├── Charge Scheduler A's Time
      │
      └── Charge Root's Time
```

This introduces hierarchical accounting but avoids requiring each thread to possess a separately funded CPU-time allocation.

### The main drawback

Naively charging every level introduces an O(depth) operation for each accounting update.

Also, a scheduler cannot meaningfully execute its children without receiving execution capacity from its parent.

Those problems can be addressed with an *effective execution limit*.

For example:

```text
Current thread allowance:    5 ms
Parent allowance:            3 ms
Grandparent allowance:      10 ms
```

The next execution interval is limited to:

\[
\Delta t = \min(5,3,10)=3\text{ ms}
\]

The kernel only needs to program a timer for the nearest limit.

It can defer some accounting operations until a context switch, rather than updating every accounting object on every timer tick.

For very deep scheduling hierarchies, some per-level accounting is still unavoidable if every level requires exact usage information. Depth limits, cached effective deadlines, and lazy accounting would help.

**My preference would be a hybrid of Designs A and B:** independently identifiable `Time` accounting objects, with explicit authority to fund or limit execution and an optional parent-controlled scheduling domain.

This permits both client-funded RPC and hierarchical scheduling without making every timeslice a newly allocated capability.

---

# 7. Scheduler hierarchies: the kernel should support them, but not prescribe them

I would make `Scheduler` objects nestable.

However, I wouldn't necessarily model a scheduler as a thread that repeatedly receives CPU time and then manually yields to its children.

That can introduce unnecessary context switches.

Instead, think of the scheduler as a **domain of scheduling authority**, which happens to be managed by a userspace thread.

```text
                    Root Scheduler
                          │
              ┌───────────┴───────────┐
              │                       │
         Scheduler A              Scheduler B
              │                       │
       EDF policy                  MuQSS policy
              │                       │
        ┌─────┴─────┐           ┌─────┴─────┐
        │           │           │           │
      Thread 1    Thread 2    Thread 3    Thread 4
```

A useful capability distinction emerges:

- A `Scheduler` capability grants authority over a scheduling domain.
- A `Time` capability grants access to a particular accounting or execution resource.
- A `Thread` capability grants authority over the thread itself.

These rights should remain independent.

For example, a filesystem server might receive a client's `Time` authority through IPC without receiving authority to modify the client's scheduler.

Similarly, a scheduler might be authorized to run a thread without being allowed to modify the thread's address space or registers.

## 7.1 A scheduler needs explicit authority over CPUs

A scheduler shouldn't automatically be permitted to execute threads on any CPU.

I'd associate every scheduling domain with an allowed CPU set:

```rust
struct Scheduler {
    allowed_cpus: CpuSet,
    parent: Option<SchedulerId>,
}
```

A child scheduler's CPU authority must be a subset of its parent's.

One possibility is to make CPU scheduling authority a capability in its own right, but I wouldn't introduce that object immediately.

It might be sufficient for a `Scheduler` capability to carry CPU-set restrictions.

For example:

```text
Scheduler A:
    CPUs 0–3

Scheduler B:
    CPUs 2–3
```

This supports overlapping scheduling domains provided their execution grants are coordinated through their common parent.

On a particular CPU, the kernel must still enforce exclusive ownership of the active dispatch slot.

## 7.2 Don't require a context switch at every hierarchy level

Suppose:

```text
Root Scheduler
    ↓
Scheduler A
    ↓
Scheduler B
    ↓
Thread X
```

A naive implementation might require:

```text
Root runs
    → A runs
        → B runs
            → X runs
```

Then, when X blocks:

```text
X → B → A → Root
```

This is expensive.

I'd prefer allowing a parent to delegate execution authority to a child scheduler such that the child can directly dispatch its own threads.

The root only needs to become involved when:

- The child exhausts its allocated execution allowance.
- The child explicitly yields its allocation.
- The parent revokes the child's execution authority.
- A scheduling event requires the parent's attention.

The kernel can cache the currently effective execution allowance, avoiding repeated parent scheduler activations.

This is where scheduling domains become more useful than simply having scheduler threads call one another.

---

# 8. A better `Time` object: separate accounting and allowance

I'd consider structuring the `Time` object approximately like this:

```rust
struct Time {
    consumed: u64,
    available: u64,

    // Internal kernel state
    active_execution: Option<ExecutionId>,
}
```

But I would be careful about the interpretation of `available`.

There are two quantities worth distinguishing:

**Consumption:** How much CPU execution has been charged to this object?

**Allowance:** How much additional execution is currently authorized?

Consumption is monotonically increasing, modulo counter representation.

Allowance can increase through authorized grants and decrease through consumption.

These are logically separate, even if implemented in one object.

### Possible interface

```rust
Time::consumed() -> Duration

Time::remaining() -> Duration

Time::transfer(
    destination: TimeCap,
    amount: Duration,
)

Time::reclaim(
    source: TimeCap,
    amount: Duration,
)
```

Transfers should require the corresponding rights.

An ordinary thread holding a `Time` capability for accounting purposes must not automatically have permission to withdraw or transfer the remaining budget.

I would use attenuated capability rights:

| Right | Meaning |
|---|---|
| Observe | Read consumption and remaining allowance |
| Spend | Authorize execution against the object |
| Transfer | Move available allowance to another object |
| Configure | Change permitted accounting configuration |
| Reclaim | Recover unused delegated allowance, subject to revocation rules |

This is only a conceptual rights model; I wouldn't necessarily implement five separate rights immediately.

The important part is separating *observing consumption* from *controlling execution authority*.

### A subtle security property

When a thread executes using a `Time` object, it should not necessarily gain possession of a capability to that object.

Otherwise, a server executing on a client's donated budget might acquire the ability to transfer or modify that budget.

Instead, the kernel should support something like:

```text
Execution context:
    Thread = FilesystemServer
    Time   = ClientA
```

The server can consume the client's allowance without acquiring management authority over it.

This is another reason to keep `Thread`, `Time`, and `Scheduler` independent.

---

# 9. Budget donation: explicit versus implicit

There are at least three designs.

## Option A: automatic donation on synchronous IPC

Similar to seL4 MCS passive servers.

```text
Client A
    |
    | synchronous call
    ▼
Server S

Time A automatically follows execution.
```

Advantages:

- Efficient.
- Natural for synchronous RPC.
- Client CPU accounting follows the request.
- No userspace scheduler round trip is necessary for ordinary IPC.

Disadvantages:

- Kernel IPC semantics become coupled to scheduling.
- Nested calls need explicit unwind rules.
- Server budget exhaustion requires careful handling.
- Asynchronous requests don't fit naturally.

seL4 MCS uses passive servers and reply objects to manage scheduling-context donation along synchronous IPC chains. Its kernel also handles sporadic replenishment, which is one respect in which your proposed architecture could push more policy into userspace. [seL4 Documentation](https://docs.sel4.systems/Tutorials/mcs.html?utm_source=chatgpt.com)

## Option B: explicit donation

Make the scheduling-context transfer explicit:

```rust
ipc.call(
    server,
    message,
    donate_time = client_time,
);
```

Or:

```rust
Time::lend(server_thread);
```

Now donation becomes an independent operation.

Advantages:

- Clearer authority boundaries.
- Applicable beyond conventional RPC.
- The caller can choose whether to donate.
- Works with scheduling-context capabilities already in use.

Disadvantages:

- The kernel still needs to track ownership and restoration.
- More complicated call-site semantics.
- Potentially additional capability manipulation.

## Option C: scheduler-mediated donation

On IPC, the kernel simply reports that the client is waiting for the server.

The scheduler determines whether to transfer time.

```text
A calls S
    ↓
Scheduler notified
    ↓
Scheduler assigns A's Time to S
    ↓
S executes
```

This is arguably the purest mechanism-not-policy design.

Unfortunately, it introduces a userspace scheduling round trip on the critical IPC path.

For a microkernel with frequent synchronous calls, that could be prohibitively expensive.

### My choice: explicit authorization, automatic execution transfer

I'd use a hybrid.

A client explicitly authorizes budget donation as part of a synchronous call.

The kernel then implements the transfer and restoration without invoking the userspace scheduler on every IPC transition.

Something like:

```rust
ipc.call(
    server,
    message,
    IpcFlags::DONATE_TIME,
);
```

The budget travels with the synchronous call chain.

For asynchronous execution, I'd provide a separate explicit mechanism.

The important boundary is that the kernel implements **transfer of an already authorized execution resource**, not any particular policy for deciding how much budget the client should receive.

---

# 10. Scheduler activation is itself a scheduling problem

This is the biggest bootstrapping problem.

Suppose CPU 0 is executing thread A.

Thread B becomes runnable.

The scheduler needs to run to decide whether B should preempt A.

But the scheduler itself is a userspace thread.

Who schedules the scheduler?

You cannot solve this by asking the same scheduler to decide when it should execute.

There has to be some mechanism above it.

I see three choices.

## Option A: privileged scheduler activation

The kernel immediately activates the appropriate userspace scheduler when it needs a decision.

Scheduler execution is treated specially.

```text
Application thread
       │
       │ scheduling event
       ▼
Kernel
       │
       ▼
Userspace scheduler
       │
       │ dispatch(...)
       ▼
Application thread
```

This resembles a scheduler activation or upcall mechanism.

The kernel doesn't choose which application thread to run.

It merely ensures that the responsible scheduler receives execution.

The problem is resource accounting.

If scheduler execution is effectively unlimited, a malicious scheduler could monopolize the CPU.

## Option B: scheduler execution consumes its own budget

Give the scheduler a `Time` object.

```text
Scheduler S:
    Time = 1 ms / 10 ms
```

Scheduler activation consumes its own budget.

This is easy to understand, but has a serious drawback.

If the scheduler exhausts its budget, its entire domain can stop making progress.

This may be acceptable for an untrusted child scheduler but not for the root scheduler.

It also introduces awkward interactions between scheduling decisions and replenishment.

## Option C: parent-funded scheduler execution

This is my preferred approach for hierarchical scheduling.

Every child scheduler is scheduled by its parent.

```text
Root Scheduler
      │
      │ funds scheduler execution
      ▼
Child Scheduler
      │
      │ allocates application execution
      ▼
Application Threads
```

The parent controls both:

- CPU time consumed by the child's applications.
- CPU time consumed by the child scheduler itself.

This avoids free scheduler execution.

But at the root, the kernel still needs a privileged activation path.

I'd make the root dispatcher a small, special mechanism rather than pretending that the entire scheduling hierarchy can be implemented without any kernel scheduling rules.

### A proposed kernel invariant

**Every CPU always has a designated controlling scheduler, and the kernel can activate that scheduler when a scheduling decision is required.**

The root scheduler's activation authority is established at boot.

Child schedulers receive restricted activation authority from their parents.

The kernel needs only enough built-in behavior to ensure that the appropriate scheduler can regain control, subject to its execution allowance.

This is analogous to having a minimal trap handler for scheduling decisions rather than a full in-kernel scheduling policy.

---

# 11. What about timer management?

I'd keep deadlines and replenishment entirely in userspace, but provide a general timer mechanism.

A scheduler needs to request activation at an absolute time:

```rust
Scheduler::arm_timer(
    deadline: Instant,
);
```

For example, an EDF scheduler can ask to be awakened at its next job release.

A sporadic-server scheduler can request activation at its next replenishment.

A MuQSS implementation can request a scheduling decision at its next relevant virtual deadline.

The kernel doesn't need to know why the timer exists.

There are two distinct timers:

| Timer | Purpose | Controlled by |
|---|---|---|
| Execution timer | Preempt a running thread when its grant expires | Kernel |
| Policy timer | Activate a scheduler at a requested wall-clock time | Userspace |

These should be conceptually independent, even if implemented through the same hardware timer.

For efficiency, the kernel can program the next hardware interrupt as:

\[
t_{\text{interrupt}} =
\min(
t_{\text{execution limit}},
t_{\text{scheduler alarm}}
)
\]

On a multicore system, this calculation is performed for the relevant CPU.

I would make policy timers use absolute monotonic timestamps rather than durations. This simplifies periodic releases, deadline scheduling, and handling delayed activations.

---

# 12. Multicore scheduling: centralized and per-CPU schedulers

One desirable property is to support both designs without requiring separate kernel interfaces.

### Centralized scheduler

```text
             Scheduler
            /    |    \
           /     |     \
        CPU 0  CPU 1  CPU 2
```

A single userspace scheduler makes decisions for all CPUs.

This is attractive for:

- Global EDF.
- Global MuQSS-like scheduling.
- Small multicore systems.
- Centralized load balancing.

But it can become a contention point.

### Per-CPU schedulers

```text
Scheduler 0     Scheduler 1     Scheduler 2
    |               |               |
  CPU 0           CPU 1           CPU 2
```

Each CPU has an independent scheduler.

They can communicate to migrate threads or steal work.

This is attractive for:

- Scalable fair schedulers.
- CPU-local real-time scheduling.
- Cache-affinity-aware policies.

### Supporting both

I'd let a `Scheduler` capability authorize dispatch operations for a CPU set.

The scheduler can have one or multiple userspace agents.

The kernel doesn't need to distinguish centralized from distributed scheduling policies.

However, a centralized scheduler should be able to issue a batch of dispatch decisions:

```rust
Scheduler::commit(&[
    Dispatch { cpu: 0, thread: a, ... },
    Dispatch { cpu: 1, thread: b, ... },
    Dispatch { cpu: 2, thread: c, ... },
]);
```

Batching reduces syscall overhead and can help coordinate migrations. It doesn't necessarily imply that all remote CPUs change execution simultaneously.

I'd make this an optimization added after the single-CPU path works.

---

# 13. The design I'd actually implement

After considering the alternatives, I would start with a smaller architecture than the full proposal above.

In particular, I'd resist introducing a complicated transferable time-credit economy immediately.

Instead, I'd implement **execution grants** as the fundamental mechanism.

## Kernel objects

### `Thread`

Holds execution state and a reference to its scheduling domain.

### `Time`

Holds:

```rust
struct Time {
    consumed: u64,
    remaining: u64,
}
```

The object can be used independently of any particular thread.

Execution charges can be attributed to it, and its allowance can be modified only through authorized operations.

### `Scheduler`

Defines a scheduling domain.

Holds:

```rust
struct Scheduler {
    controlling_agent: ThreadId,
    parent: Option<SchedulerId>,
    allowed_cpus: CpuSet,
    event_queue: EventQueue,
}
```

No priorities, deadline trees, run queues, or fairness data structures need to live in the kernel.

### `ExecutionGrant`

I would initially make this **transient kernel state**, not a separate capability object.

```rust
struct ExecutionGrant {
    thread: ThreadId,
    time: TimeId,
    scheduler: SchedulerId,
    cpu: CpuId,
    limit: Duration,
}
```

The grant exists while a thread is executing, and the kernel accounts CPU consumption against its `Time`.

The implementation could simply store this in per-CPU state.

I would not expose a separate `ExecutionGrant` capability unless a real use case emerges for transferring outstanding grants independently of their associated `Time` objects.

## Minimal operations

I think the initial ABI could be surprisingly small.

| Operation | Purpose |
|---|---|
| `Scheduler::attach(thread)` | Assign thread to scheduler domain |
| `Scheduler::detach(thread)` | Remove scheduling authority |
| `Scheduler::dispatch(thread, time, limit)` | Execute a thread |
| `Scheduler::replace(cpu, thread, time, limit)` | Atomically replace current execution |
| `Scheduler::wait()` | Receive scheduling events |
| `Scheduler::arm_timer(deadline)` | Request timed activation |
| `Time::read()` | Observe consumption and allowance |
| `Time::grant(amount)` | Add allowance using authorized funding |
| `Time::transfer(destination, amount)` | Transfer funded allowance |

The `Time::grant` operation needs a precise authority model: it must not allow an untrusted scheduler to manufacture additional execution capacity. For the first implementation, I'd restrict it to the root scheduling authority or to transferring capacity from an existing parent allocation.

For hierarchical schedulers, add delegation of scheduling authority and effective execution limits. You needn't implement a fully general hierarchy on day one, but the capability model should not prevent it.

### One very useful additional mechanism: direct handoff

I'd strongly consider:

```rust
Scheduler::handoff(
    from: ThreadCap,
    to: ThreadCap,
    time: TimeCap,
    limit: Duration,
)
```

This would atomically suspend or block one thread and execute another using the specified time resource.

Ordinary synchronous IPC could use a specialized fast path for this.

It would support:

- Client-funded RPC.
- Coroutine-style thread handoff.
- Work delegation.
- Fast userspace scheduling transitions.

A general scheduler could authorize the handoff in advance, rather than being invoked on every RPC.

---

# 14. Some important traps to avoid

There are a few architectural issues I would address early.

### 14.1 Don't make scheduler events lossy

If a scheduler misses a `Runnable` notification, a thread might never execute again.

Events must either be reliably delivered or accompanied by authoritative state that the scheduler can inspect.

I particularly like ghOSt's approach of combining event delivery with kernel-maintained status information and sequence numbers.

A useful design is:

```rust
struct ThreadStatus {
    generation: u64,
    state: ThreadState,
    consumed: u64,
}
```

A shared-memory status page could expose this without requiring a syscall for every accounting query.

The kernel would publish state transitions with appropriate memory ordering. Sequence numbers let the scheduler detect stale observations.

You still need an overflow protocol for event queues, but lost notifications no longer have to mean lost thread state.

### 14.2 Make scheduler operations generation-safe

A thread might block, wake, migrate, or terminate while a scheduling decision is being prepared.

A scheduler should never accidentally dispatch a thread based on stale state.

Use generation counters and atomic validation at dispatch time.

### 14.3 Define what happens when a scheduler crashes

Suppose a scheduler controls 100 threads and terminates unexpectedly.

Possible recovery policies include:

- Suspend all controlled threads.
- Return scheduling control to the parent.
- Activate a designated replacement scheduler.

I'd implement the first two mechanisms in the kernel.

The parent can decide whether to restart the scheduler, migrate its threads, or terminate its domain.

A crashed root scheduler is a separate failure: no purely userspace policy can recover unless the kernel already has a trusted emergency activation or replacement mechanism.

### 14.4 Don't confuse available CPU time with guaranteed service

A scheduler may have permission to consume 5 ms without being guaranteed 5 ms of actual execution within a particular interval.

To express guarantees, the parent must also provide a temporal service contract.

For example:

```text
Child scheduler:
    5 ms execution capacity per 10 ms interval
```

A child with an ordinary allowance of 5 ms cannot infer that guarantee merely from `Time::remaining() == 5 ms`.

This matters if you eventually want compositional real-time scheduling.

### 14.5 Account for kernel execution time

You should explicitly define whether CPU time spent executing syscalls, handling IPC, and performing kernel work is charged to the currently active `Time`.

I'd generally charge synchronous kernel work to the active execution context.

Otherwise, an application could consume CPU without exhausting its allowance by generating excessive kernel activity.

Interrupt and deferred-work accounting is harder.

A device interrupt may not be attributable to a particular client. Kernel mechanisms should at least distinguish such execution, even if the attribution policy is initially simple.

### 14.6 Time exhaustion during synchronous IPC

Consider:

```text
Client A
   ↓
Filesystem Server
   ↓
Device Driver
```

All three execute using A's time resource.

What happens if the budget expires inside the device driver?

One possible semantic is to suspend the entire execution chain until A's allowance is replenished.

But that can cause priority inversion or block unrelated clients if shared state is held.

I'd distinguish between a normal exhaustion notification and a terminal failure.

The scheduler or resource manager should be able to replenish an exhausted context without forcing the server to abandon the request.

But a passive server must also support explicit cancellation and recovery.

The kernel alone cannot guarantee temporal isolation if the server holds shared locks while suspended.

---

# 15. Can the proposed mechanisms actually implement the different policies?

This is the test I'd use to evaluate the design.

| Policy | Userspace data structures | Required kernel mechanisms |
|---|---|---|
| FIFO | Priority queues | Dispatch, preempt |
| Round-robin | FIFO queues, quantum | Dispatch, exhaustion event |
| MuQSS | Skip lists, virtual deadlines | Dispatch, retained slice accounting |
| EEVDF | Eligible tree, lag, virtual deadlines | Accurate execution accounting, dispatch |
| EDF | Deadline queue | Timers, preemption |
| Sporadic server | Replenishment queue | Accounting, timers, execution limits |
| SCHED_ISO-like | CPU usage history, fallback priority | Accounting, dispatch |
| Weighted fair | Weights, accumulated service | Accounting, dispatch |
| Hierarchical scheduler | Nested scheduling domains | Delegation, bounded execution |
| Client-funded RPC | Donation chain | Time handoff |
| Partitioned RT | Per-partition budgets | Domain-level execution limits |
| Global EDF | Global runnable queue | Multicore dispatch and migration |

The important question isn't whether a policy is easy to implement.

It's whether implementing it requires changing the kernel.

If you can implement all these policies using the same basic mechanisms, the abstraction boundary is probably in the right place.

Two reservations are worth noting. First, achieving high-performance implementations of all these policies may require optimized shared-memory dispatch and event interfaces. Second, implementing a policy is not the same as obtaining its theoretical real-time guarantees; those depend on the parent scheduler's service guarantees and system assumptions.

---

# 16. One more radical alternative: remove `Time` from dispatch entirely

There's another design worth contemplating.

What if the kernel interface were simply:

```rust
Scheduler::dispatch(
    thread: ThreadCap,
    duration: Duration,
)
```

And `Time` were attached to the **current execution chain** rather than explicitly passed with every dispatch?

For example:

```text
Client A runs
    Time = A

A calls Server S
    Time remains A

S calls Driver D
    Time remains A
```

The kernel maintains a current execution context:

```rust
struct ExecutionContext {
    thread: ThreadId,
    time: TimeId,
    scheduler: SchedulerId,
}
```

A normal scheduler dispatch reuses the time authority associated with the selected thread or execution continuation.

An explicit operation is required only when changing the time authority:

```rust
Thread::bind_time(time)
```

Or:

```rust
Time::donate(thread)
```

This could make ordinary dispatch cheaper and simplify accounting across RPC.

However, it also risks coupling time authority to thread state in ways that become awkward for asymmetric scheduling or concurrent requests.

I'd therefore lean toward retaining explicit `Time` selection in the conceptual dispatch interface, while allowing the kernel to optimize common cases through cached bindings and IPC handoff.

---

# 17. What I'd borrow from existing research

Three systems are particularly relevant.

**seL4 MCS** demonstrates that CPU execution budgets can be represented as separate capability-controlled objects and propagated across synchronous IPC. Its implementation keeps fixed-priority scheduling and sporadic replenishment in the kernel, so your design could go further by delegating those policies to userspace. [seL4 MCS](https://docs.sel4.systems/Tutorials/mcs.html)

**ghOSt** is probably the closest reference for actual userspace scheduling policy delegation. Its event interface, shared status information, generation-based validation, and transactional dispatch are directly relevant. Importantly, it demonstrates policies ranging from centralized FIFO to EDF without requiring each policy to be implemented as a new kernel scheduler. [ghOSt paper](https://research.google/pubs/ghost-fast-and-flexible-user-space-delegation-of-linux-scheduling/) [Google Research](https://research.google/pubs/ghost-fast-and-flexible-user-space-delegation-of-linux-scheduling/?utm_source=chatgpt.com)

**Scheduler Activations** (Anderson et al., 1992) is a foundational reference for kernel notifications of scheduling-relevant events and cooperation between kernel and userspace schedulers. It addresses several problems that reappear whenever scheduling decisions move out of the kernel. [Scheduler Activations](https://doi.org/10.1145/146941.146944) [DOI](https://doi.org/10.1145/146941.146944?utm_source=chatgpt.com)

I'd study ghOSt particularly closely before finalizing the dispatch ABI. Its handling of races between kernel events and userspace decisions is highly relevant to keeping the fast path efficient.

---

# 18. My preferred architecture

If I had to choose an initial design, it would look like this:

```text
                       ┌───────────────────┐
                       │ Root Scheduler    │
                       │                   │
                       │ Userspace policy  │
                       └─────────┬─────────┘
                                 │
                      delegates authority
                                 │
              ┌──────────────────┴──────────────────┐
              │                                     │
              ▼                                     ▼
    ┌───────────────────┐                ┌───────────────────┐
    │ Scheduler A       │                │ Scheduler B       │
    │                   │                │                   │
    │ EDF / Real-time   │                │ MuQSS / Fair      │
    └─────────┬─────────┘                └─────────┬─────────┘
              │                                     │
        dispatch()                              dispatch()
              │                                     │
       ┌──────┴──────┐                       ┌──────┴──────┐
       │             │                       │             │
       ▼             ▼                       ▼             ▼
    Thread A1     Thread A2               Thread B1     Thread B2
       │             │                       │             │
       ▼             ▼                       ▼             ▼
     Time A1       Time A2                 Time B1       Time B2
```

The kernel contains only:

```text
Thread state
Scheduling authority
Execution accounting
Execution limits
CPU dispatch
Scheduler activation
Event notification
Timer management
```

Everything else is userspace policy.

I'd introduce the mechanisms in three stages.

**Stage 1 — Basic userspace scheduling.** Implement `Thread`, `Scheduler`, and `Time`, plus dispatch, blocking/waking notifications, accurate CPU accounting, and execution timers. Use a single root scheduler initially. Test FIFO, RR, MuQSS-like, and EDF policies.

**Stage 2 — Temporal isolation and RPC.** Introduce separately enforceable execution allowances, authorized time transfer, and synchronous IPC donation. Test shared passive servers and exhaustion recovery.

**Stage 3 — Hierarchical and multicore scheduling.** Add delegated scheduler domains, parent-enforced execution limits, per-CPU scheduling agents, and batched remote dispatch.

This sequence gives you a useful kernel early without requiring you to solve hierarchical real-time scheduling before having a functional scheduler.

## One architectural decision I'd make now

I'd distinguish between these two concepts, even if they initially share one object:

- **`Time` is the accounting and execution-resource identity.**
- **`Scheduler` is the authority to decide how that resource gets spent.**

A scheduler can spend only time it is authorized to spend. A thread can execute using time it doesn't own. A time resource can move between threads without changing their scheduling authority.

That produces a nice symmetry with capability-based memory management: possession of a resource capability and authority to select how that resource is used are related but distinct.

I'd probably keep the name **`Time`**, rather than `Budget`, if the object supports accounting, transfer, and execution attribution. A budget is a quantity or limit; `Time` can represent a first-class computational resource. `Budget` might be the name of one field or operation on it.

**The hardest unresolved design decision is the relationship between a scheduler's own CPU execution and the CPU time it allocates to other threads.** I'd resolve that before settling the ABI. If scheduler execution and application execution share a parent-enforced allowance, hierarchical scheduling becomes relatively natural. If scheduler execution is privileged and unaccounted, you can still implement userspace policies, but strong temporal isolation becomes much harder to guarantee.

My central design preference is therefore:

> Keep policy decisions in userspace, make execution grants the kernel's fundamental scheduling primitive, and enforce CPU-time authority at scheduling-domain boundaries rather than embedding particular replenishment algorithms in the kernel.
