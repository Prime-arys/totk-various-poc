#include "regen.hpp"

#include "game.hpp"
#include "options.hpp"

#include <algorithm>
#include <atomic>

/* The entry of the hook (entry.s), which runs `enemy_hp_enemy_gauge` below
   with the game's floating point registers put aside. */
extern "C" void enemy_hp_enemy_gauge_entry(exl::hook::InlineCtx* ctx);

namespace regen {

    namespace {

        /* The ordinary enemy gauge, just after it fetched the life component.
           What 1.2.1 has from 8 bytes before: bl <life component>, cbz x0,
           mov x21, x0 (replaced), str w23, [sp, #8]. */
        constexpr game::Site EnemyGauge {
            0x12c7a48,
            { 0x5b, 0x58, 0xe0, 0x97, 0xc0, 0x06, 0x00, 0xb4, 0xf5, 0x03, 0x00, 0xaa, 0xf7, 0x0b, 0x00, 0xb9 },
        };

        /* A gauge update after a pause (a menu, the map) must not heal for the
           whole pause: time is counted by update, at most this much each. */
        constexpr double MaxStep = 0.25;
        /* Enemies followed at once; more gauges than this are never on screen. */
        constexpr size_t Tracked = 64;

        struct Enemy {
            uintptr_t m_Life;
            s32 m_LastSeen;
            double m_LastDamage;
            double m_LastUpdate;
            /* Life earned but not handed out yet (less than one point). */
            float m_Fraction;
        };

        /* The gauge code runs on the game's own threads, which must never be
           made to wait on one of the SDK's locks: a spin lock it is. */
        std::atomic<bool> s_Busy { false };
        Enemy s_Enemies[Tracked];
    }

    bool Update(uintptr_t life, Gauge gauge, s32* raised, s32* max_life) {
        const options::Values* values = options::Get();
        if (values == nullptr || life == 0)
            return false;
        const float percent = gauge == Gauge::Boss ? values->m_BossRegenPercent : values->m_RegenPercent;
        if (percent <= 0.0f)
            return false;

        s32* cell = game::CurrentLife(life);
        if (cell == nullptr)
            return false;
        const s32 current = game::LoadLife(cell);
        const s32 max     = game::MaxLife(life);
        if (current <= 0 || max <= 0)
            return false;

        while (s_Busy.exchange(true, std::memory_order_acquire))
            __asm__ __volatile__("yield");
        const double time = game::Seconds();

        /* This enemy's entry, or the one updated longest ago. */
        Enemy* enemy  = nullptr;
        Enemy* oldest = &s_Enemies[0];
        for (auto& entry : s_Enemies) {
            if (entry.m_Life == life) {
                enemy = &entry;
                break;
            }
            if (entry.m_LastUpdate < oldest->m_LastUpdate)
                oldest = &entry;
        }
        if (enemy == nullptr) {
            enemy  = oldest;
            *enemy = { life, current, time, time, 0.0f };
        }

        if (current < enemy->m_LastSeen) {
            /* It was hit: the wait starts over. */
            enemy->m_LastDamage = time;
            enemy->m_Fraction   = 0.0f;
        }
        const double step   = std::clamp(time - enemy->m_LastUpdate, 0.0, MaxStep);
        enemy->m_LastUpdate = time;

        s32 now = current;
        if (current < max && time - enemy->m_LastDamage >= static_cast<double>(values->m_RegenDelay)) {
            enemy->m_Fraction += static_cast<float>(max) * percent / 100.0f * static_cast<float>(step);
            const s32 gained = static_cast<s32>(enemy->m_Fraction);
            if (gained >= 1) {
                const s32 target = std::min(current + gained, max);
                /* Lost to a hit landing right now: that hit wins, and the next
                   update sees it. */
                if (game::RaiseLife(cell, current, target)) {
                    enemy->m_Fraction -= static_cast<float>(gained);
                    now = target;
                }
            }
        } else if (current >= max) {
            enemy->m_Fraction = 0.0f;
        }
        enemy->m_LastSeen = now;

        s_Busy.store(false, std::memory_order_release);

        if (now <= current)
            return false;
        if (raised != nullptr)
            *raised = now;
        if (max_life != nullptr)
            *max_life = max;
        return true;
    }

    bool Found() {
        return game::Matches(EnemyGauge);
    }

    void Install() {
        exl::hook::HookInline(game::Base() + EnemyGauge.m_Offset, enemy_hp_enemy_gauge_entry);
    }
}

/* Called for each shown enemy gauge, every frame, with the enemy's life
   component in x0. */
extern "C" void enemy_hp_enemy_gauge(exl::hook::InlineCtx* ctx) {
    regen::Update(ctx->X[0], regen::Gauge::Enemy, nullptr, nullptr);
}
