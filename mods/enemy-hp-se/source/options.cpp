#include "options.hpp"

#include "lib.hpp"
#include "nn.hpp"

#include <algorithm>
#include <atomic>
#include <iterator>
#include <string_view>

namespace options {

    namespace {

        /* Where the game mounts its romfs (the name it gives nn::fs::MountRom). */
        constexpr const char* Files[] = {
            "content:/EnemyHp/Regen.txt",
            "content:/EnemyHp/RegenDelay.txt",
            "content:/EnemyHp/RegenBoss.txt",
        };

        enum : u32 { Unread, Reading, Ready };
        std::atomic<u32> s_State { Unread };

        /* The defaults: what the package's default options say. */
        Values s_Values { 1.5f, 0.2f, 8.0f };
        bool s_Regen           = true;
        bool s_RegenBosses     = true;
        bool s_BossesAsEnemies = false;

        std::string_view Trim(std::string_view text) {
            auto blank = [](char c) { return c == ' ' || c == '\t' || c == '\r'; };
            while (!text.empty() && blank(text.front()))
                text.remove_prefix(1);
            while (!text.empty() && blank(text.back()))
                text.remove_suffix(1);
            return text;
        }

        /* "1.5" as well as "1,5"; nothing else is a number. */
        bool ParseNumber(std::string_view text, float* out) {
            float value = 0.0f;
            float scale = 0.0f;
            bool digits = false;
            for (char c : text) {
                if (c >= '0' && c <= '9') {
                    digits = true;
                    if (scale == 0.0f) {
                        value = value * 10.0f + static_cast<float>(c - '0');
                    } else {
                        value += static_cast<float>(c - '0') * scale;
                        scale /= 10.0f;
                    }
                } else if ((c == '.' || c == ',') && scale == 0.0f) {
                    scale = 0.1f;
                } else {
                    return false;
                }
            }
            if (digits)
                *out = value;
            return digits;
        }

        bool IsOff(std::string_view value) {
            return value == "0" || value == "false" || value == "no" || value == "off";
        }

        /* The same keys as the Skyline edition's enemy-hp.ini. */
        void Apply(std::string_view key, std::string_view value) {
            float number = 0.0f;
            if (key == "regen") {
                s_Regen = !IsOff(value);
            } else if (key == "regen_bosses") {
                s_RegenBosses = !IsOff(value);
            } else if (key == "regen_percent") {
                if (ParseNumber(value, &number))
                    s_Values.m_RegenPercent = number;
            } else if (key == "regen_delay") {
                if (ParseNumber(value, &number))
                    s_Values.m_RegenDelay = number;
            } else if (key == "regen_percent_bosses") {
                if (value == "same") {
                    s_BossesAsEnemies = true;
                } else if (ParseNumber(value, &number)) {
                    s_Values.m_BossRegenPercent = number;
                    s_BossesAsEnemies           = false;
                }
            }
        }

        void Parse(std::string_view text) {
            while (!text.empty()) {
                auto end  = text.find('\n');
                auto line = text.substr(0, end);
                text      = end == std::string_view::npos ? std::string_view {} : text.substr(end + 1);

                line = line.substr(0, line.find_first_of("#;"));
                auto equals = line.find('=');
                if (equals == std::string_view::npos)
                    continue;
                Apply(Trim(line.substr(0, equals)), Trim(line.substr(equals + 1)));
            }
        }

        /* Reads a small text file whole; false when it is not there. */
        bool ReadFile(const char* path, char* buffer, size_t capacity, size_t* length) {
            nn::fs::FileHandle handle;
            if (R_FAILED(nn::fs::OpenFile(&handle, path, nn::fs::OpenMode_Read)))
                return false;

            long size = 0;
            bool read = R_SUCCEEDED(nn::fs::GetFileSize(&size, handle)) && size > 0;
            if (read) {
                /* Never more than the file holds: nn::fs takes that badly. */
                *length = std::min(static_cast<size_t>(size), capacity);
                read    = R_SUCCEEDED(nn::fs::ReadFile(handle, 0, buffer, *length));
            }
            nn::fs::CloseFile(handle);
            return read;
        }

        void Load() {
            int found = 0;
            for (const char* path : Files) {
                char buffer[256];
                size_t length = 0;
                if (ReadFile(path, buffer, sizeof(buffer), &length)) {
                    Parse({ buffer, length });
                    found++;
                }
            }

            s_Values.m_RegenPercent = std::clamp(s_Values.m_RegenPercent, 0.0f, 100.0f);
            if (s_BossesAsEnemies)
                s_Values.m_BossRegenPercent = s_Values.m_RegenPercent;
            s_Values.m_BossRegenPercent = std::clamp(s_Values.m_BossRegenPercent, 0.0f, 100.0f);
            s_Values.m_RegenDelay       = std::max(s_Values.m_RegenDelay, 0.0f);
            if (!s_Regen)
                s_Values.m_RegenPercent = s_Values.m_BossRegenPercent = 0.0f;
            if (!s_RegenBosses)
                s_Values.m_BossRegenPercent = 0.0f;

            /* Hundredths, so that no floating point formatting is pulled in. */
            auto hundredths = [](float value) { return static_cast<int>(value * 100.0f + 0.5f); };
            Logging.Log("[enemy-hp-se] options (%d of %d files): enemies %d.%02d%%/s, bosses %d.%02d%%/s, after %d.%02ds",
                found, static_cast<int>(std::size(Files)),
                hundredths(s_Values.m_RegenPercent) / 100, hundredths(s_Values.m_RegenPercent) % 100,
                hundredths(s_Values.m_BossRegenPercent) / 100, hundredths(s_Values.m_BossRegenPercent) % 100,
                hundredths(s_Values.m_RegenDelay) / 100, hundredths(s_Values.m_RegenDelay) % 100);
        }
    }

    const Values* Get() {
        if (s_State.load(std::memory_order_acquire) == Ready)
            return &s_Values;

        /* The gauges run on the game's threads: whoever comes while the files
           are being read goes without for a frame rather than wait. */
        u32 expected = Unread;
        if (!s_State.compare_exchange_strong(expected, Reading, std::memory_order_acq_rel))
            return nullptr;
        Load();
        s_State.store(Ready, std::memory_order_release);
        return &s_Values;
    }
}
