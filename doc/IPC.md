From [Gernot's blog](https://microkerneldude.wordpress.com/2019/03/07/how-to-and-how-not-to-use-sel4-ipc/)

> IPC is a user-controlled context switch with benefits.
> - Chuck Gray

You switch to a different thread (usually in a different address space), without involving the scheduler.
The _benefit_ is that you get to carry along a bit of data.

> IPC is the seL4 mechanism for implementing cross-domain function calls.

Present mainline has the clients represented implicitly, but this being fixed with 
the introduction of explicit reply capabilities in the [new
mixed-criticality (MCS) kernel](https://ts.data61.csiro.au/projects/TS/realtime.pml) that is making its way through verification,
and will eventually replace mainline. The MCS kernel makes invocation more like
a function call, by providing passive servers, effectively resulting in
*a migrating-threads model*.
