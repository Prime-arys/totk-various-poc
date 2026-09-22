#define NX_SERVICE_ASSUME_NON_DOMAIN
#include "core/sysclk.hpp"

#include <sys/stat.h>

#include <algorithm>
#include <cstdio>

#include <switch.h>

#include "util/log.hpp"

namespace sysclk
{

namespace
{
    const char* SERVICE = "sys:clk";

    enum Command : u32
    {
        GetApiVersion     = 0,
        GetCurrentContext = 2,
        SetOverride       = 8,
        GetFreqList       = 11,
    };

    enum Module : u32
    {
        Cpu    = 0,
        Gpu    = 1,
        Memory = 2,
        Count  = 3,
    };

    /** SysClkContext of API version 4 (sys-clk 2.x). */
    struct ContextV4
    {
        u8 enabled;
        u64 applicationId;
        u32 profile;
        u32 freqs[Count];
        u32 realFreqs[Count];
        u32 overrideFreqs[Count];
        u32 temps[3];
        s32 power[2];
        u32 ramLoad[2];
    };
    static_assert(sizeof(ContextV4) == 88, "sys-clk's context layout");

    struct Session
    {
        Service service{};
        bool open = false;

        Session()
        {
            open = running() && R_SUCCEEDED(smGetService(&service, SERVICE));
        }

        ~Session()
        {
            if (open)
                serviceClose(&service);
        }

        /** Asking sm for a service nobody registered waits forever: find out
         *  first, the way sys-clk's own client does. sm refuses to register a
         *  name that is taken; any other refusal says nothing either way.
         *  Only when sys-clk is installed: emulators handle these requests
         *  badly (Ryujinx crashes unregistering the name). */
        static bool running()
        {
            struct stat info;
            if (stat("sdmc:/atmosphere/contents/00FF0000636C6BFF/exefs.nsp", &info) != 0)
                return false;
            Handle handle    = INVALID_HANDLE;
            SmServiceName nm = smEncodeName(SERVICE);
            Result rc        = smRegisterService(&handle, nm, false, 1);
            if (R_SUCCEEDED(rc))
            {
                smUnregisterService(nm);
                svcCloseHandle(handle);
                return false;
            }
            const Result ALREADY_REGISTERED = MAKERESULT(21, 4);
            return R_VALUE(rc) == ALREADY_REGISTERED;
        }

        u32 apiVersion()
        {
            u32 version = 0;
            if (R_FAILED(serviceDispatchOut(&service, GetApiVersion, version)))
                return 0;
            return version;
        }

        std::vector<u32> frequencies(Module module)
        {
            u32 list[32]  = {};
            u32 count     = 0;
            struct
            {
                u32 module;
                u32 maxCount;
            } args = { module, 32 };
            Result rc = serviceDispatchInOut(&service, GetFreqList, args, count,
                .buffer_attrs = { SfBufferAttr_HipcAutoSelect | SfBufferAttr_Out },
                .buffers      = { { list, sizeof(list) } }, );
            std::vector<u32> result;
            if (R_SUCCEEDED(rc))
                result.assign(list, list + std::min<u32>(count, 32));
            std::sort(result.begin(), result.end());
            return result;
        }

        Result setOverride(Module module, u32 hz)
        {
            struct
            {
                u32 module;
                u32 hz;
            } args = { module, hz };
            return serviceDispatchIn(&service, SetOverride, args);
        }
    };

    bool active              = false;
    bool touched[Count]      = {};
    u32 previous[Count]      = {};

    std::string mhz(u32 hz)
    {
        char text[16];
        std::snprintf(text, sizeof(text), "%u", hz / 1000000);
        return text;
    }
} // namespace

Info query()
{
    Info info;
    Session session;
    if (!session.open)
        return info;
    info.running    = true;
    info.apiVersion = session.apiVersion();
    info.cpu        = session.frequencies(Cpu);
    info.memory     = session.frequencies(Memory);
    return info;
}

uint32_t pick(const std::vector<uint32_t>& list, uint32_t wantedMhz)
{
    uint32_t wanted = wantedMhz * 1000000u;
    if (list.empty())
        return wanted;
    uint32_t best = list.front();
    for (uint32_t hz : list)
        // Frequencies such as 1331.2 MHz are listed to the hertz.
        if (hz / 1000000u <= wantedMhz)
            best = hz;
    return best;
}

std::string boost(uint32_t cpuHz, uint32_t memoryHz)
{
    unboost();
    Session session;
    if (!session.open)
        return "";

    // Overrides someone set already (the overlay) come back afterwards. Only
    // version 4's context layout is known.
    std::fill(std::begin(previous), std::end(previous), 0);
    if (session.apiVersion() == 4)
    {
        ContextV4 context{};
        if (R_SUCCEEDED(serviceDispatchOut(&session.service, GetCurrentContext, context)))
            std::copy(std::begin(context.overrideFreqs), std::end(context.overrideFreqs), previous);
    }

    std::string done;
    auto set = [&](Module module, u32 hz, const char* name) {
        if (hz == 0)
            return;
        Result rc = session.setOverride(module, hz);
        if (R_FAILED(rc))
        {
            char text[64];
            std::snprintf(text, sizeof(text), "sys-clk refused the %s override: 0x%X", name, rc);
            applog::write(text);
            return;
        }
        touched[module] = true;
        active          = true;
        done += std::string(done.empty() ? "" : ", ") + name + " " + mhz(hz) + " MHz";
    };
    set(Cpu, cpuHz, "CPU");
    set(Memory, memoryHz, "memory");
    if (!done.empty())
        applog::write("sys-clk: " + done + " while merging");
    return done;
}

void unboost()
{
    if (!active)
        return;
    active = false;
    Session session;
    for (u32 module = 0; module < Count; module++)
    {
        if (!touched[module])
            continue;
        touched[module] = false;
        if (session.open)
            session.setOverride((Module)module, previous[module]);
    }
    applog::write(session.open ? "sys-clk: overrides put back" : "sys-clk: could not put the overrides back");
}

} // namespace sysclk
