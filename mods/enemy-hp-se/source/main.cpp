/* enemy-hp-se: the code of the Enemy HP mod, as a module of the game's exefs.

   The Skyline edition (plugins/enemy-hp) is a plugin, loaded by skyline-totk
   and set through a file next to it. This one is what TKMM calls a mod's
   code: a subsdk built with exlaunch, shipped inside the .tkcl package,
   loaded by the console itself when the game starts. It needs neither
   Skyline nor the merger, and its settings are the options of the package
   (options.hpp).

   It does what the plugin does, minus the report:

   - regeneration (regen.cpp): enemies whose gauge is shown get their life
     back over time, after a few seconds without being hit; bosses and
     mini-bosses too;
   - the numbers under the boss gauge (boss.cpp), which the game has no code
     for.

   The numbers over an ordinary enemy's health bar are not this module's
   doing: the game writes them, and the package supplies what it lacks (the
   VisualizeLife armour effect, the text panes, the messages). What the module
   knows of the game's code is in game.hpp. */

#include "lib.hpp"

#include "boss.hpp"
#include "game.hpp"
#include "regen.hpp"

extern "C" void exl_main(void* x0, void* x1) {
    /* Nothing is set up for hooking unless the game is the one read: on any
       other build the module stays a dead weight of a few kilobytes. */
    const bool enemies = regen::Found();
    const bool bosses  = boss::Found();
    if (enemies || bosses) {
        exl::hook::Initialize();
        if (enemies)
            regen::Install();
        if (bosses)
            boss::Install();
    }
    Logging.Log("[enemy-hp-se] regeneration %s, boss gauge %s",
        enemies ? "on (enemy gauge hooked)" : "off: the game's code at 0x12c7a48 is not 1.2.1's, or is hooked already",
        bosses ? "hooked" : "left alone: the game's code at 0x1ae22b0 is not 1.2.1's, or is hooked already");
}

extern "C" NORETURN void exl_exception_entry() {
    /* Only reached in an applet or a system module, which this is not. */
    EXL_ABORT("Default exception handler called!");
}
