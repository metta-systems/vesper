## Supported Syscalls

Kernel supports a very small number of syscalls. They are related to IPC and support blocking and non-blocking invocations. Other types of kernel APIs are implemented as "invocations on capability", similar to seL4. As such they are not different from services provided by other out-of-kernel servers.

List of kernel syscalls:

  * Send
  * NBSend
  * Call
  * Recv
  * Reply
  * ReplyRecv
  * NBRecv
  * Yield
  * (VMEnter)

Additional syscalls that only exist in debug builds:

  * DebugPutChar
  * DebugHalt
  * DebugSnapshot
  * DebugCapIdentify
  * DebugNameThread
  * DebugRun

Additional syscalls that only exist in benchmarking builds:

  * BenchmarkFlushCaches
  * BenchmarkResetLog
  * BenchmarkFinalizeLog
  * BenchmarkSetLogBuffer
  * BenchmarkGetThreadUtilisation
  * BenchmarkResetThreadUtilisation
  * BenchmarkNullSyscall

Kernel APIs implemented as capability invocations:

  * Untyped.Retype()

  * TCB.ReadRegisters()
  * TCB.WriteRegisters()
  * TCB.CopyRegisters()
  * TCB.Configure()
  * TCB.SetPriority()
  * TCB.SetMCPriority()
  * TCB.SetIPCBuffer()
  * TCB.SetSpace()
  * TCB.Suspend()
  * TCB.Resume()
  * TCB.BindNotification()
  * TCB.UnbindNotification()
  * TCB.SetAffinity()
  * TCB.SetBreakpoint()
  * TCB.GetBreakpoint()
  * TCB.UnsetBreakpoint()
  * TCB.ConfigureSingleStepping()

  * CNode.Revoke()
  * CNode.Delete()
  * CNode.CancelBadgedSends()
  * CNode.Copy()
  * CNode.Mint()
  * CNode.Move()
  * CNode.Mutate()
  * CNode.Rotate()
  * CNode.SaveCaller()

  * IRQControl.Get()

  * IRQHandler.Ack()
  * IRQHandler.SetNotification()
  * IRQHandler.Clear()

  * DomainSet.Set()

  * Xxx.SendEvent()

## TODO

Do we need any extra kernel entities like Events/Notifications for e.g. fbufs? - in Nemesis sending an Event is the only IPC method available - arguments and results are marshalled in the IPC shared memory area.

Thread migration - the threads are not migrating per se, the capabilities ensure that "migration" is transparent.

### Send

Send to a capability

 * @param[in] dest The capability to be invoked.
 * @param[in] msgInfo The `message_info_t` structure for the IPC.

### NBSend

Perform a polling send to a capability

 * @param[in] dest The capability to be invoked.
 * @param[in] msgInfo The `message_info_t` structure for the IPC.

### Call

Call a capability

 * @param[in] dest The capability to be invoked.
 * @param[in] msgInfo The `message_info_t` structure for the IPC.

 * @return A `message_info_t` structure

### Recv

Block until a message is received on an endpoint

 * @param[in] src The capability to be invoked.
 * @param[out] sender The address to write sender information to.
              The sender information is the badge of the
              endpoint capability that was invoked by the
              sender, or the notification word of the
              notification object that was signalled.
              This parameter is ignored if `NULL`.

 * @return A `message_info_t` structure

### Reply

Perform a send to a one-off reply capability stored when
the thread was last called

 * @param[in] msgInfo The `message_info_t` structure for the IPC.

### ReplyRecv

Perform a reply followed by a receive in one system call

 * @param[in] dest The capability to be invoked.
 * @param[in] msgInfo The `message_info_t` structure for the IPC.
 * @param[out] sender The address to write sender information to.
 *               The sender information is the badge of the
 *               endpoint capability that was invoked by the
 *               sender, or the notification word of the
 *               notification object that was signalled.
 *               This parameter is ignored if `NULL`.

 * @return A `message_info_t` structure

### NBRecv

Receive a message from an endpoint but do not block
in the case that no messages are pending

 * @param[in] src The capability to be invoked.
 * @param[out] sender The address to write sender information to.
 *                    The sender information is the badge of the
 *                    endpoint capability that was invoked by the
 *                    sender, or the notification word of the
 *                    notification object that was signalled.
 *                    This parameter is ignored if `NULL`.

 * @return A `message_info_t` structure

### Yield

Donate the remaining timeslice to a thread of the same priority
 
### (VMEnter)

static inline seL4_Word
seL4_VMEnter(seL4_CPtr vcpu, seL4_Word *sender)


## Pseudo-syscalls

### Signal

Signal a notification

This is not a proper system call known by the kernel. Rather, it is a
convenience wrapper which calls seL4_Send().
It is useful for signalling a notification.

 * @param[in] dest The capability to be invoked.

### Wait

Perform a receive on a notification object

This is not a proper system call known by the kernel. Rather, it is a
convenience wrapper which calls seL4_Recv().

 * @param[in] src The capability to be invoked.
 * @param[out] sender The address to write sender information to.
 *               The sender information is the badge of the
 *               endpoint capability that was invoked by the
 *               sender, or the notification word of the
 *               notification object that was signalled.
 *               This parameter is ignored if `NULL`.

### Poll

Perform a non-blocking recv on a notification object

This is not a proper system call known by the kernel. Rather, it is a
convenience wrapper which calls seL4_NBRecv().
It is useful for doing a non-blocking wait on a notification.

 * @param[in] src The capability to be invoked.
 * @param[out] sender The address to write sender information to.
 *               The sender information is the badge of the
 *               endpoint capability that was invoked by the
 *               sender, or the notification word of the
 *               notification object that was signalled.
 *               This parameter is ignored if `NULL`.
