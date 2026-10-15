#include "boss.hpp"

#include "game.hpp"
#include "regen.hpp"

#include <algorithm>
#include <atomic>

/* The entry of the hook (entry.s), which runs `enemy_hp_boss_gauge` below
   with the game's floating point registers put aside. */
extern "C" void enemy_hp_boss_gauge_entry(exl::hook::InlineCtx* ctx);

namespace boss {

    namespace {

        /* Right after the boss gauge fetched the life component. What 1.2.1
           has from 8 bytes before: bl <life component>, cbz x0, mov x20, x0
           (replaced), ldr x0, [sp, #16]. */
        constexpr game::Site BossGauge {
            0x1ae22b0,
            { 0x41, 0xee, 0xbf, 0x97, 0xe0, 0x09, 0x00, 0xb4, 0xf4, 0x03, 0x00, 0xaa, 0xe0, 0x0b, 0x40, 0xf9 },
        };

        /* The screen's layout, which the game hands to the same text function
           when it writes the boss's name (0x1ae2660). */
        constexpr ptrdiff_t LayoutField = 40;
        /* The three animations that draw the bar, which the game moves
           together when the gauge fills in (0x1ae2228). Each frame the game
           puts the first where the life says, and lets the others catch up -
           but only while the "damage" animation is over. Life given back
           changes the life every frame, which keeps that animation going, so
           the bar would stay where the last hit left it: the three are moved
           from here instead. */
        constexpr ptrdiff_t BarAnimations[] = { 416, 424, 448 };
        /* Set when the gauge is a long one; the animation then covers the
           whole bar instead of half of it (0x1ae2394). */
        constexpr ptrdiff_t LongGaugeField = 512;
        /* In an animation: the resource it plays (its frame count is a u16 at
           +8), the frame it is on, and SetFrame in its vtable. */
        constexpr ptrdiff_t AnimationResource = 24;
        constexpr ptrdiff_t AnimationFrame    = 32;
        constexpr ptrdiff_t SetFrameMethod    = 208;
        /* The pane the mod's layout adds under the bar. */
        constexpr char Pane[] = "T_BossLife_00";
        /* A text written is written again after this long even if it did not
           change, in case the layout was reset under it. */
        constexpr double RefreshSeconds = 1.0;
        /* Boss gauges on screen at once. */
        constexpr size_t Screens = 4;

        /* What was last written into a screen's pane. */
        struct Shown {
            uintptr_t m_Screen;
            s32 m_Current;
            s32 m_Max;
            bool m_Visible;
            double m_Time;
        };

        /* Only the game's UI thread comes here, but a spin lock costs nothing. */
        std::atomic<bool> s_Busy { false };
        Shown s_Shown[Screens];
        std::atomic<bool> s_NoPaneSaid { false };

        /* Puts the bar where the life is, as the game does when the gauge
           fills in. */
        void MoveBar(uintptr_t screen, float ratio) {
            auto first = game::Read<uintptr_t>(screen + BarAnimations[0]);
            if (first == 0)
                return;
            auto resource = game::Read<uintptr_t>(first + AnimationResource);
            if (resource == 0)
                return;
            const float frames = static_cast<float>(game::Read<u16>(resource + 8));
            const bool whole   = game::Read<u8>(screen + LongGaugeField) != 0;
            const float frame  = std::clamp(ratio, 0.0f, 1.0f) * frames * (whole ? 1.0f : 0.5f);
            for (ptrdiff_t field : BarAnimations) {
                auto animation = game::Read<uintptr_t>(screen + field);
                if (animation == 0 || game::Read<float>(animation + AnimationFrame) == frame)
                    continue;
                auto vtable = game::Read<uintptr_t>(animation);
                using SetFrame = void (*)(uintptr_t, float);
                reinterpret_cast<SetFrame>(game::Read<uintptr_t>(vtable + SetFrameMethod))(animation, frame);
            }
        }

        /* Writes `value` (0 or more) at `at`, returns where it ends. */
        u32 PushNumber(char16_t* text, u32 at, s32 value) {
            char16_t digits[10];
            u32 count = 0;
            u32 rest  = static_cast<u32>(std::max(value, 0));
            do {
                digits[count++] = static_cast<char16_t>(u'0' + rest % 10);
                rest /= 10;
            } while (rest != 0);
            while (count != 0)
                text[at++] = digits[--count];
            return at;
        }

        /* Writes the boss's life under its gauge, when it changed. */
        void Show(uintptr_t screen, uintptr_t life) {
            auto layout = game::Read<uintptr_t>(screen + LayoutField);
            if (layout == 0)
                return;
            const bool visible = game::VisualizesLife();
            const s32* cell    = game::CurrentLife(life);
            const s32 current  = std::max(cell != nullptr ? game::LoadLife(cell) : 0, 0);
            const s32 max      = std::max(game::MaxLife(life), 0);

            while (s_Busy.exchange(true, std::memory_order_acquire))
                __asm__ __volatile__("yield");
            const double time = game::Seconds();
            Shown* entry      = nullptr;
            Shown* oldest     = &s_Shown[0];
            for (auto& candidate : s_Shown) {
                if (candidate.m_Screen == screen) {
                    entry = &candidate;
                    break;
                }
                if (candidate.m_Time < oldest->m_Time)
                    oldest = &candidate;
            }
            const bool same = entry != nullptr && entry->m_Visible == visible
                && (!visible || (entry->m_Current == current && entry->m_Max == max));
            if (same && time - entry->m_Time < RefreshSeconds) {
                s_Busy.store(false, std::memory_order_release);
                return;
            }
            if (entry == nullptr)
                entry = oldest;
            *entry = { screen, current, max, visible, time };
            s_Busy.store(false, std::memory_order_release);

            /* "current/max", or nothing without a VisualizeLife armour. */
            char16_t text[32];
            u32 length = 0;
            if (visible && max > 0) {
                length         = PushNumber(text, 0, current);
                text[length++] = u'/';
                length         = PushNumber(text, length, max);
            }
            text[length] = 0;

            if (game::SetPaneText(layout, Pane, text, length) == 0 && !s_NoPaneSaid.exchange(true))
                Logging.Log("[enemy-hp-se] the boss gauge has no %s pane: the layout of the mod is not the one served", Pane);
        }
    }

    bool Found() {
        return game::Matches(BossGauge);
    }

    void Install() {
        exl::hook::HookInline(game::Base() + BossGauge.m_Offset, enemy_hp_boss_gauge_entry);
    }
}

/* Called for each boss gauge, every frame, with the boss's life component in
   x0 and the gauge's screen in x19. */
extern "C" void enemy_hp_boss_gauge(exl::hook::InlineCtx* ctx) {
    const uintptr_t life   = ctx->X[0];
    const uintptr_t screen = ctx->X[19];
    if (life == 0 || screen == 0)
        return;

    s32 current = 0;
    s32 max     = 0;
    if (regen::Update(life, regen::Gauge::Boss, &current, &max))
        boss::MoveBar(screen, static_cast<float>(current) / static_cast<float>(max));
    boss::Show(screen, life);
}
