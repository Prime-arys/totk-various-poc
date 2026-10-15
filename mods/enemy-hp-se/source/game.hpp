#pragma once

/* What the mod knows of Tears of the Kingdom 1.2.1's code, in one place.

   Every address here is an offset from the start of the game's main module,
   found by reading 1.2.1's code (the same ones as the Skyline edition,
   plugins/enemy-hp/src/game.rs, where the reading is told at length). A hook
   is only placed after checking that the bytes around the spot are the ones
   read, so another build of the game is left untouched. */

#include "lib.hpp"

#include <cstring>

namespace game {

    /* A place in the game's code to hook, and what 1.2.1 has from 8 bytes
       before it: the instruction replaced must be one that can be moved. */
    struct Site {
        ptrdiff_t m_Offset;
        u8 m_Expected[16];
    };

    /* Where the game's main module starts. */
    inline uintptr_t Base() {
        return exl::util::modules::GetTargetStart();
    }

    /* Whether the game's code at `site` is the one read in 1.2.1. Also false
       once something else has hooked the same instruction: two hooks there
       would regenerate twice. */
    inline bool Matches(const Site& site) {
        const auto& text = exl::util::GetMainModuleInfo().m_Text;
        if (site.m_Offset < 8 || static_cast<size_t>(site.m_Offset) + 8 > text.m_Size)
            return false;
        auto found = reinterpret_cast<const void*>(Base() + site.m_Offset - 8);
        return std::memcmp(found, site.m_Expected, sizeof(site.m_Expected)) == 0;
    }

    template<typename T>
    inline T Read(uintptr_t address) {
        return *reinterpret_cast<const volatile T*>(address);
    }

    /* --- an actor's life ---------------------------------------------------

       The life component, as the game's damage code (0x64b4b8) and its gauges
       read it. Each field points at something holding an s32 at +8:

         [[life + 6192] + 8]  current life, written with ldxr/stxr
         [[life + 6200] + 8]  maximum life
         [[life + 6216] + 8]  life taken off the maximum (when the pointer is set)
         [[life + 6208] + 8]  a second pool that soaks damage before the life,
                              empty for ordinary enemies - not the life, left alone */
    constexpr ptrdiff_t CurrentLifeField = 6192;
    constexpr ptrdiff_t MaxLifeField     = 6200;
    constexpr ptrdiff_t MaxLifeLossField = 6216;

    /* The s32 at +8 of what [life + field] points at, or 0. */
    inline s32 Value(uintptr_t life, ptrdiff_t field) {
        auto holder = Read<uintptr_t>(life + field);
        return holder != 0 ? Read<s32>(holder + 8) : 0;
    }

    /* The current life, to read and change the way the game does (with
       exclusive loads and stores), or nullptr. */
    inline s32* CurrentLife(uintptr_t life) {
        auto holder = Read<uintptr_t>(life + CurrentLifeField);
        return holder != 0 ? reinterpret_cast<s32*>(holder + 8) : nullptr;
    }

    inline s32 LoadLife(const s32* cell) {
        return __atomic_load_n(cell, __ATOMIC_ACQUIRE);
    }

    /* Raises the life from `expected` to `target`; false when a hit landed in
       between, which then wins. */
    inline bool RaiseLife(s32* cell, s32 expected, s32 target) {
        return __atomic_compare_exchange_n(cell, &expected, target, false, __ATOMIC_ACQ_REL, __ATOMIC_ACQUIRE);
    }

    /* The most life the actor can have right now. */
    inline s32 MaxLife(uintptr_t life) {
        return Value(life, MaxLifeField) - Value(life, MaxLifeLossField);
    }

    /* --- the numbers -------------------------------------------------------

       The byte the enemy gauges test (0x12c78cc) before they show their
       numbers: set while the armour worn has the VisualizeLife effect. It
       sits in an object the game keeps a pointer to at this address. */
    constexpr ptrdiff_t PlayerState        = 0x462ec80;
    constexpr ptrdiff_t VisualizeLifeField = 2204;

    inline bool VisualizesLife() {
        auto global = Read<uintptr_t>(Base() + PlayerState);
        if (global == 0)
            return false;
        auto state = Read<uintptr_t>(global);
        return state != 0 && Read<u8>(state + VisualizeLifeField) != 0;
    }

    /* What the game's text functions take (built by 0x1235fc4 from a string):
       the text, its length in UTF-16 units, and a message attribute (-1: none). */
    struct Message {
        const char16_t* m_Text;
        u32 m_Length;
        s64 m_Attribute;
    };

    /* 0xb519b0(layout, pane name, message, refresh, 0): finds the text pane of
       that name in a layout and gives it the text. The boss gauge writes its
       name with it. Returns how many panes took the text. */
    constexpr ptrdiff_t SetPaneTextFunction = 0xb519b0;

    inline u32 SetPaneText(uintptr_t layout, const char* pane, const char16_t* text, u32 length) {
        Message message { text, length, -1 };
        using Function = u32 (*)(uintptr_t, const char*, const Message*, u32, uintptr_t);
        return reinterpret_cast<Function>(Base() + SetPaneTextFunction)(layout, pane, &message, 1, 0);
    }

    /* --- time --------------------------------------------------------------

       Seconds, from the counter the console's own clocks are built on. Read
       straight from the processor: nothing of the game or the SDK is needed,
       so it works from the first instruction of the module on. */
    inline double Seconds() {
        u64 ticks, frequency;
        __asm__ __volatile__("mrs %[ticks], cntpct_el0" : [ticks] "=r"(ticks));
        __asm__ __volatile__("mrs %[frequency], cntfrq_el0" : [frequency] "=r"(frequency));
        return frequency != 0 ? static_cast<double>(ticks) / static_cast<double>(frequency) : 0.0;
    }
}
