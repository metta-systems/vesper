# Capability Derivation Tree

Derived capabilities maintain the relationship between them and the source capability.

Derivation relation is recorded in the Capability Derivation Tree. It is finite but possibly arbitrarily long chain.

An LCD can grant a capability to another LCD.
Suppose LCD A grants a capability to LCD B.
When this happens, the cnode in LCD A's cspace becomes the parent of
the cnode in LCD B's cspace, in a capability derivation tree, or CDT.
If LCD B grants the rights on the same object to LCD C,
the cnode in LCD C's cspace becomes a child of LCD B's cnode in the cdt,
and so on. If LCD A decides to revoke rights, the microkernel will recursively
delete the capabilities in LCD B's and LCD C's cspaces, using the CDT.

```
   - LCD A's cspace -         
   |                |           - LCD B's cspace -
   |     .          |           |                |
   |    .           |           |                |
   |   /            |           |                |
   | +-+            |           |     .          |
   | +-+            |           |      .         |
   `- \ ------------'           |       \        |
       `------------------------------> +-+      |
       |                        |       +-+      |
       |                         `-------|-------'
       |      - LCD C's cspace -         |
       |      |                |         |
       |      |   .            |         |    - LCD D's cspace -
       |      |    \           |         |    |                |
       `---------> +-+         |         |    |        .       |
              |    +-+         |         |    |       .        |
              `----------------'         |    |      /         |
                                         |    |     +-+        |
                                          `-------> +-+        |
                                              `----------------'

```




Physical memory is divided into untyped objects
- Authority conferred via capabilities
- Untyped capability is sufficient authority to allocate kernel objects

Allocate on explicit user request
- Creator gets the full authority
- Distribute capabilities to allow other access the service

(https://ess.cs.tu-dortmund.de/workshops/iies/2008/presentations/seL4-iies08.pdf)


Formal proof:

Proof also identify the invariants the “supervisory OS” needs to enforce
for isolation to hold:
- Can not share modifiable page/capability tables
- Can not share thread control blocks
- Can not have communication channels that allow capability propagation

