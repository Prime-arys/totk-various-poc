#pragma once

/* The options chosen for the mod.

   A module has no settings file of its own: what TKMM (or the mod manager)
   lets the player choose are the options of the package, and an option can
   only bring files. So each option group of EnemyHpSe.tkcl ships one small
   text file in romfs, which the merged game serves like any other:

     content:/EnemyHp/Regen.txt       regen_percent = 1.5
     content:/EnemyHp/RegenDelay.txt  regen_delay = 8
     content:/EnemyHp/RegenBoss.txt   regen_percent_bosses = 0.2   (or "same")

   They are read once, the first time a gauge needs them: by then the game has
   long mounted its files. A file that is missing leaves its default. */

namespace options {

    struct Values {
        /* Share of the maximum life regained per second, in percent; 0: none. */
        float m_RegenPercent;
        /* The same for a boss or mini-boss, which has far more life. */
        float m_BossRegenPercent;
        /* Seconds without losing life before regeneration starts. */
        float m_RegenDelay;
    };

    /* The options once they have been read, nullptr until then (and for a
       thread that asks while another one is reading them). */
    const Values* Get();
}
