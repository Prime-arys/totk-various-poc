/* What the game's code enters when it reaches a hooked instruction.

   exlaunch's inline hooks put the general purpose registers aside before
   calling back, and nothing else. The two gauges hooked here are in the
   middle of drawing: they may well hold something in a floating point
   register across the hooked instruction, and what runs from the callback
   (the arithmetic of regeneration, the game's own text and animation
   functions) is free to use every one of them. Skyline's inline hooks, which
   the plugin edition of this mod runs on, save q0 to q31 for that reason; so
   do these entries, around the C++ callback.

   x0 (the context exlaunch hands over) is not touched on the way in. */

.macro FLOAT_SAFE_ENTRY name, callback
    .section .text.\name, "ax", %progbits
    .global \name
    .hidden \name
    .type \name, %function
    .align 2
\name:
    stp x29, x30, [sp, #-0x10]!
    mov x29, sp
    sub sp, sp, #0x200
    stp q0, q1, [sp, #0x000]
    stp q2, q3, [sp, #0x020]
    stp q4, q5, [sp, #0x040]
    stp q6, q7, [sp, #0x060]
    stp q8, q9, [sp, #0x080]
    stp q10, q11, [sp, #0x0A0]
    stp q12, q13, [sp, #0x0C0]
    stp q14, q15, [sp, #0x0E0]
    stp q16, q17, [sp, #0x100]
    stp q18, q19, [sp, #0x120]
    stp q20, q21, [sp, #0x140]
    stp q22, q23, [sp, #0x160]
    stp q24, q25, [sp, #0x180]
    stp q26, q27, [sp, #0x1A0]
    stp q28, q29, [sp, #0x1C0]
    stp q30, q31, [sp, #0x1E0]

    bl \callback

    ldp q0, q1, [sp, #0x000]
    ldp q2, q3, [sp, #0x020]
    ldp q4, q5, [sp, #0x040]
    ldp q6, q7, [sp, #0x060]
    ldp q8, q9, [sp, #0x080]
    ldp q10, q11, [sp, #0x0A0]
    ldp q12, q13, [sp, #0x0C0]
    ldp q14, q15, [sp, #0x0E0]
    ldp q16, q17, [sp, #0x100]
    ldp q18, q19, [sp, #0x120]
    ldp q20, q21, [sp, #0x140]
    ldp q22, q23, [sp, #0x160]
    ldp q24, q25, [sp, #0x180]
    ldp q26, q27, [sp, #0x1A0]
    ldp q28, q29, [sp, #0x1C0]
    ldp q30, q31, [sp, #0x1E0]
    add sp, sp, #0x200
    ldp x29, x30, [sp], #0x10
    ret
    .size \name, . - \name
.endm

FLOAT_SAFE_ENTRY enemy_hp_enemy_gauge_entry, enemy_hp_enemy_gauge
FLOAT_SAFE_ENTRY enemy_hp_boss_gauge_entry, enemy_hp_boss_gauge
