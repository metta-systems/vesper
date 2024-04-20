# Architecture-specific code

This directory contains code specific to a certain architecture.

Implementations of arch-specific kernel calls are also placed here.

One of the submodules will be exported based conditionally on target_arch. Currently, the code depending on it will import specific architecture explicitly, there are no default reexports.

----

For more information please re-read.
