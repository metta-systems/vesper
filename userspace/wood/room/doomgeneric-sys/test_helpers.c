// Test helpers: expose C constants and struct layout values so Rust tests
// can validate them against ported Rust equivalents.
#include <stddef.h>
#include "doomdef.h"
#include "info.h"

int room_test_get_doom_191_version(void) {
    return DOOM_191_VERSION;
}

// state_t layout
int room_test_get_state_t_sizeof(void)      { return (int)sizeof(state_t); }
int room_test_get_state_t_tics_offset(void) { return (int)offsetof(state_t, tics); }

// mobjinfo_t layout
int room_test_get_mobjinfo_t_sizeof(void)        { return (int)sizeof(mobjinfo_t); }
int room_test_get_mobjinfo_t_speed_offset(void)  { return (int)offsetof(mobjinfo_t, speed); }

// statenum_t enum values used by G_SetFastMonsters
int room_test_get_s_sarg_run1(void)  { return (int)S_SARG_RUN1; }
int room_test_get_s_sarg_pain2(void) { return (int)S_SARG_PAIN2; }

// mobjtype_t enum values used by G_SetFastMonsters
int room_test_get_mt_bruisershot(void) { return (int)MT_BRUISERSHOT; }
int room_test_get_mt_headshot(void)    { return (int)MT_HEADSHOT; }
int room_test_get_mt_troopshot(void)   { return (int)MT_TROOPSHOT; }
