#pragma once

/* Enemies around the player get their life back over time.

   "Around the player" is what the game itself decides: the enemies whose
   health gauge it shows. Two gauges call in here, once per frame each:

   - the gauge over an ordinary enemy (those fighting Link or close to him -
     further away with VisualizeLife). At 0x12c7a48 the game has just fetched
     the enemy's life component (in x0), right before reading its life to draw
     the bar and the numbers; the hook is placed there;
   - the big gauge of a boss or mini-boss, which boss.cpp hooks and hands over.

   Either way the life is topped up before the gauge reads it, so the bar and
   the numbers show the regained life on the same frame. A dead enemy (0 life)
   is never brought back, life never goes past the maximum, and regeneration
   waits a few seconds after the enemy last lost life, so a fight is not
   undone as it happens. */

#include "lib.hpp"

namespace regen {

    /* Which gauge an update comes from: bosses have a share of their own. */
    enum class Gauge { Enemy, Boss };

    /* Whether the game's code at the ordinary enemy gauge is the one of
       1.2.1, and not hooked already. */
    bool Found();

    /* Hooks that gauge; only once Found() said so and exlaunch's hooks are
       initialized. */
    void Install();

    /* Tops up the life of the actor whose life component is `life`, if it is
       its time. Called once per frame per shown gauge; true when it did raise
       the life, which is then in `raised` (and the maximum in `max`). */
    bool Update(uintptr_t life, Gauge gauge, s32* raised, s32* max);
}
