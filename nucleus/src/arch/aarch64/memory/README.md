# Memory Configuration

The types VirtAddr and PhysAddr are representing the addresses before and after the mapping in the MMU.

Page table types must represent pages of differing sizes.
For every entry in the MMU page table we should be able to receive a proper page type - e.g. Invalid, further page table, or
a specific-size page.
