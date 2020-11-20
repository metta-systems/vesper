/*
 * SPDX-License-Identifier: BlueOak-1.0.0
 * Copyright (c) Berkus Decker <berkus+vesper@metta.systems>
 */
// ARM GIC control
//
// Provide access to enable, route interrupts in general on ARM.
// This is for RasPi4 and other boards with GIC.
//
// RPI3 DOES NOT HAVE A GENERIC ARM IC, BUT A CUSTOM BROADCOM ONE
// RPI4 HAS A STANDARD GIC
//
// 1. What we want to get running first is a generic timer interrupt.
//
