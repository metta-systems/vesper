// Paging is mostly based on https://os.phil-opp.com/page-tables/ and ARM ARM

// AArch64:
// Table D4-8-2021: check supported granule sizes, select alloc policy based on results.
// TTBR_ELx is the pdbr for specific page tables

// Page 2068 actual page descriptor formats

