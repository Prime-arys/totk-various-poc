#pragma once

/* The big gauge of bosses and mini-bosses (Hinox, Stone Talus, Molduga,
   Gleeok, the temples' bosses...): the numbers under it, and regeneration.

   TotK 1.2.1 updates that gauge (its UIBossLifeScreen, 0x1ae2178) once per
   frame: it fetches the boss's life component and reads its current and
   maximum life to fill the bar. A hook right after the fetch (0x1ae22b0, life
   component in x0, the screen in x19) regenerates the boss, then writes
   "current/max" into the text pane T_BossLife_00 that the mod's layout adds
   under the bar. Unlike the ordinary enemy gauge, the game has no code of its
   own for boss numbers.

   Like the enemy numbers, they only show while the armour worn has the
   VisualizeLife effect (the Champion's Tunic, or any armour, depending on the
   mod's option). */

namespace boss {

    /* Whether the game's code at the boss gauge is the one of 1.2.1, and not
       hooked already. */
    bool Found();

    /* Hooks that gauge; only once Found() said so and exlaunch's hooks are
       initialized. */
    void Install();
}
