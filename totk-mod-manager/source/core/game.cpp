#include "core/game.hpp"

#include <sys/stat.h>

#include <cstdio>
#include <cstdlib>
#include <cstring>

#include <switch.h>

#include "core/core.hpp"
#include "util/log.hpp"

namespace game
{

namespace
{
    const char* MOUNT_NAME = "game";
    const char* SD_DUMP    = "sdmc:/totk/romfs/";
    bool mounted           = false;
    bool boosted           = false;

    std::string hex(Result rc)
    {
        char text[16];
        std::snprintf(text, sizeof(text), "0x%X", rc);
        return text;
    }

    bool looksLikeTotk(const std::string& prefix, Romfs& romfs)
    {
        core::RomInfo info = core::romInfo(prefix);
        if (!info.ok)
        {
            romfs.error = info.error;
            return false;
        }
        romfs.prefix  = prefix;
        romfs.version = info.version;
        return true;
    }
} // namespace

bool isApplication()
{
    AppletType type = appletGetAppletType();
    return type == AppletType_Application || type == AppletType_SystemApplication;
}

uint64_t currentProgramId()
{
    u64 id = 0;
    if (R_FAILED(svcGetInfo(&id, InfoType_ProgramId, CUR_PROCESS_HANDLE, 0)))
        return 0;
    return id;
}

bool runsAsTotk()
{
    return currentProgramId() == TITLE_ID;
}

Romfs mountRomfs()
{
    unmountRomfs();
    Romfs romfs;
    std::string prefix = std::string(MOUNT_NAME) + ":/";

    if (runsAsTotk())
    {
        // Started from TotK's icon with R held: Atmosphère loads the
        // homebrew menu as the game, and the file system hands out the
        // game's own romfs (base game and update) as this process's data.
        Result rc = romfsMountFromCurrentProcess(MOUNT_NAME);
        if (R_SUCCEEDED(rc))
        {
            mounted = true;
            if (looksLikeTotk(prefix, romfs))
            {
                romfs.source = RomSource::TitleOverride;
                applog::write("romfs: TotK's own (title override), version " + std::to_string(romfs.version));
                return romfs;
            }
            unmountRomfs();
        }
        else
        {
            applog::write("romfs: mounting the current process' data failed: " + hex(rc));
        }
    }

    {
        // TotK suspended in the background: its data storage is reachable
        // while its process exists.
        Result rc = romfsMountDataStorageFromProgram(TITLE_ID, MOUNT_NAME);
        if (R_SUCCEEDED(rc))
        {
            mounted = true;
            if (looksLikeTotk(prefix, romfs))
            {
                romfs.source = RomSource::RunningGame;
                applog::write("romfs: from the running game, version " + std::to_string(romfs.version));
                return romfs;
            }
            unmountRomfs();
        }
        else
        {
            applog::write("romfs: the game is not running (" + hex(rc) + ")");
        }
    }

    struct stat info;
    if (stat((std::string(SD_DUMP) + "Pack/ZsDic.pack.zs").c_str(), &info) == 0 && looksLikeTotk(SD_DUMP, romfs))
    {
        romfs.source = RomSource::SdDump;
        applog::write("romfs: extracted copy on the SD card, version " + std::to_string(romfs.version));
        return romfs;
    }

    romfs.source = RomSource::None;
    romfs.prefix.clear();
    return romfs;
}

void unmountRomfs()
{
    if (mounted)
    {
        romfsUnmount(MOUNT_NAME);
        mounted = false;
    }
}

std::string installedVersion()
{
    if (R_FAILED(nsInitialize()))
        return "";
    // The version the home menu shows, from the (updated) control data.
    std::string result;
    auto* data = (NsApplicationControlData*)malloc(sizeof(NsApplicationControlData));
    u64 size   = 0;
    if (data
        && R_SUCCEEDED(nsGetApplicationControlData(NsApplicationControlSource_Storage, TITLE_ID, data,
            sizeof(NsApplicationControlData), &size))
        && size >= sizeof(NacpStruct))
    {
        result = std::string(data->nacp.display_version, strnlen(data->nacp.display_version, sizeof(data->nacp.display_version)));
    }
    free(data);
    nsExit();
    return result;
}

std::string guessLocale()
{
    u64 code = 0;
    // The application's own choice among its languages when the manager runs
    // as the game, the system's otherwise.
    if (!(isApplication() && R_SUCCEEDED(appletGetDesiredLanguage(&code))) && R_FAILED(setGetSystemLanguage(&code)))
        return "";
    SetLanguage language;
    if (R_FAILED(setMakeLanguage(code, &language)))
        return "";
    switch (language)
    {
        case SetLanguage_JA:
            return "JPja";
        case SetLanguage_ENUS:
            return "USen";
        case SetLanguage_ENGB:
            return "EUen";
        case SetLanguage_FR:
            return "EUfr";
        case SetLanguage_FRCA:
            return "USfr";
        case SetLanguage_DE:
            return "EUde";
        case SetLanguage_IT:
            return "EUit";
        case SetLanguage_ES:
            return "EUes";
        case SetLanguage_ES419:
            return "USes";
        case SetLanguage_NL:
            return "EUnl";
        case SetLanguage_RU:
            return "EUru";
        case SetLanguage_KO:
            return "KRko";
        case SetLanguage_ZHCN:
        case SetLanguage_ZHHANS:
            return "CNzh";
        case SetLanguage_ZHTW:
        case SetLanguage_ZHHANT:
            return "TWzh";
        default:
            // Portuguese: not in TotK, which then falls back on another language.
            return "";
    }
}

void rememberGuessedLocale()
{
    struct stat info;
    if (stat("sdmc:/totk/locale.txt", &info) == 0)
        return;
    std::string locale = guessLocale();
    if (locale.empty())
        return;
    FILE* file = fopen("sdmc:/totk/locale.txt", "w");
    if (!file)
        return;
    fputs(locale.c_str(), file);
    fclose(file);
    applog::write("texts: " + locale + " guessed from the console's language (the plugin checks it at boot)");
}

bool setCpuBoost(bool on)
{
    Result rc = appletSetCpuBoostMode(on ? ApmCpuBoostMode_FastLoad : ApmCpuBoostMode_Normal);
    if (R_FAILED(rc))
    {
        applog::write(std::string("could not turn the boost mode ") + (on ? "on" : "off") + ": " + hex(rc));
        return false;
    }
    boosted = on;
    applog::write(std::string("boost mode ") + (on ? "on" : "off"));
    return true;
}

void endCpuBoost()
{
    if (boosted)
        setCpuBoost(false);
}

bool restartAfterExit(const std::string& nro)
{
    if (nro.empty() || !envHasNextLoad())
        return false;
    return R_SUCCEEDED(envSetNextLoad(nro.c_str(), nro.c_str()));
}

std::string launch()
{
    // 0 relaunches the current application, which is TotK itself when the
    // manager was started from its icon.
    u64 id    = runsAsTotk() ? 0 : TITLE_ID;
    Result rc = appletRequestLaunchApplication(id, nullptr);
    if (R_FAILED(rc))
        return hex(rc);
    return "";
}

} // namespace game
