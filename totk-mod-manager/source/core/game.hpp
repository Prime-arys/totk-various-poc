#pragma once

#include <cstdint>
#include <string>

// The game itself: where to read its romfs from for a merge, and launching it.
namespace game
{

constexpr uint64_t TITLE_ID = 0x0100F2C0115B6000;

enum class RomSource
{
    None,
    /** The manager was started from TotK's icon with R held: it runs as the
     *  game, and reads the romfs of the installed game (and its update). */
    TitleOverride,
    /** TotK is suspended in the background; its romfs is read through it. */
    RunningGame,
    /** An extracted romfs on the SD card (sd:/totk/romfs), for emulators. */
    SdDump,
};

struct Romfs
{
    RomSource source = RomSource::None;
    /** Prefix for romfs paths, e.g. "game:/". */
    std::string prefix;
    int version = 0;
    std::string error;
};

/** Where the manager runs: true when started through a game (full memory,
 *  and possibly TotK's own romfs). */
bool isApplication();
/** The program id this process runs as. */
uint64_t currentProgramId();
bool runsAsTotk();

/** Mounts the game's romfs, trying every source in the order above. */
Romfs mountRomfs();
void unmountRomfs();

/** The installed game version (e.g. "1.2.1"), or empty when it is not
 *  installed. */
std::string installedVersion();

/** The message archive locale ("EUfr"...) TotK picks for the console's
 *  language, or empty for a language it does not ship. */
std::string guessLocale();

/** Until the plugin has seen the game read its texts (sd:/totk/locale.txt),
 *  records guessLocale() there, so the first merge already skips the other
 *  languages. The plugin corrects it at boot if the guess was wrong. */
void rememberGuessedLocale();

/** Starts TotK (relaunching it when the manager runs as the game). Empty on
 *  success, otherwise what went wrong. */
std::string launch();

/** The console's boost mode, which games use while loading: the CPU runs at
 *  1785 MHz instead of 1020 (the GPU slows down meanwhile). Returns whether
 *  the system accepted it. */
bool setCpuBoost(bool on);

/** Back to normal if the boost mode is still on (on exit). */
void endCpuBoost();

/** Makes the homebrew loader start `nro` again once the manager has quit.
 *  False when not started by the homebrew loader. */
bool restartAfterExit(const std::string& nro);

} // namespace game
