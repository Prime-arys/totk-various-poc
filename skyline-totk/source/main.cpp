#include "main.hpp"

#include "skyline/logger/CompositeLogger.hpp"
#include "skyline/logger/KernelLogger.hpp"
#include "skyline/logger/SdLogger.hpp"
#include "skyline/logger/TcpLogger.hpp"
#include "skyline/utils/call_once.hpp"
#include "skyline/utils/cpputils.hpp"
#include "skyline/utils/cur_proc_handle.hpp"
#include "skyline/utils/ipc.hpp"
#include "skyline/utils/utils.h"
#include "totk/config.hpp"
#include "totk/version.hpp"

// For handling exceptions
char ALIGNA(0x1000) exception_handler_stack[0x4000];
nn::os::UserExceptionInfo exception_info;

// Tears of the Kingdom's own allocator is not usable while the subsdk's module
// initializer runs, so nothing here may touch the heap. The kernel logger only
// writes through svcOutputDebugString from a stack buffer; everything that
// allocates waits until romfs is mounted (see setup_after_romfs below).
static skyline::logger::KernelLogger g_BootLogger;

void exception_handler(nn::os::UserExceptionInfo* info) {
    skyline::logger::s_Instance->LogFormat("Exception occurred!\n");

    skyline::logger::s_Instance->LogFormat("Error description: %x\n", info->ErrorDescription);
    for (int i = 0; i < 29; i++)
        skyline::logger::s_Instance->LogFormat("X[%02i]: %" PRIx64 "\n", i, info->CpuRegisters[i].x);
    skyline::logger::s_Instance->LogFormat("FP: %" PRIx64 "\n", info->FP.x);
    skyline::logger::s_Instance->LogFormat("LR: %" PRIx64 "\n", info->LR.x);
    skyline::logger::s_Instance->LogFormat("SP: %" PRIx64 "\n", info->SP.x);
    skyline::logger::s_Instance->LogFormat("PC: %" PRIx64 "\n", info->PC.x);
    skyline::logger::s_Instance->Flush();
}

// Builds the real logger out of whatever sinks the config asked for. Runs after
// romfs is mounted, so the SD card and the game's allocator are both available.
static void setup_logger() {
    auto* logger = new skyline::logger::CompositeLogger();

    if (totk::g_Config.log_sinks & totk::Config::LogKernel) logger->AddSink(new skyline::logger::KernelLogger());

    if (totk::g_Config.log_sinks & totk::Config::LogSd)
        logger->AddSink(new skyline::logger::SdLogger(totk::g_Config.log_path));

    if (totk::g_Config.WantsTcp()) {
        skyline::logger::g_tcpPort = totk::g_Config.tcp_port;
        // Grab the socket library before the game does, then keep it from being
        // re-initialized or finalized behind our back.
        skyline::logger::skyline_socket_init();
        skyline::logger::setup_socket_hooks();
        skyline::logger::start_listen_thread();
        logger->AddSink(new skyline::logger::TcpLogger());
    }

    skyline::logger::s_Instance = logger;
    logger->StartThread();
}

static void (*VAbortImpl)(char const*, char const*, char const*, int, Result const*,
                          nn::os::UserExceptionInfo const*, char const*, va_list args);

static void handleNnDiagDetailVAbortImpl(char const* str1, char const* str2, char const* str3, int int1,
                                         Result const* code, nn::os::UserExceptionInfo const* ExceptionInfo,
                                         char const* fmt, va_list args) {
    char info[0x400] = {0};
    vsnprintf(info, sizeof(info) - 1, fmt, args);

    skyline::logger::s_Instance->LogFormat("[abort] %s\n%s\n%s\n%d\nError: 0x%x\n%s", str1, str2, str3, int1, *code,
                                          info);
    skyline::logger::s_Instance->Flush();

    VAbortImpl(str1, str2, str3, int1, code, ExceptionInfo, fmt, args);
}

// nn::diag's abort handler carries the reason a game-side assertion failed;
// hooking it turns an opaque crash into a line in the log.
static void hook_abort() {
    uintptr_t VAbort_ptr = 0;
    Result rc = nn::ro::LookupSymbol(
        &VAbort_ptr,
        "_ZN2nn4diag6detail10VAbortImplEPKcS3_S3_iPKNS_6ResultEPKNS_2os17UserExceptionInfoES3_RSt9__va_list");

    if (R_SUCCEEDED(rc) && VAbort_ptr != 0)
        A64HookFunction(reinterpret_cast<void*>(VAbort_ptr), reinterpret_cast<void*>(handleNnDiagDetailVAbortImpl),
                        (void**)&VAbortImpl);
    else
        skyline::logger::s_Instance->LogFormat("[skyline-totk] Could not hook nn::diag abort (0x%x)", rc);
}

static void setup_after_romfs() {
    nn::fs::MountSdCardForDebug("sd");
    totk::LoadConfig();

    setup_logger();
    totk::InitVersion();
    hook_abort();

    skyline::logger::s_Instance->LogFormat("[skyline-totk] Tears of the Kingdom %s (code %u)",
                                          totk::GetVersionString().c_str(), totk::GetVersionCode());
    skyline::logger::s_Instance->LogFormat("[skyline-totk] text: 0x%" PRIx64 " | rodata: 0x%" PRIx64
                                          " | data: 0x%" PRIx64 " | bss: 0x%" PRIx64 " | heap: 0x%" PRIx64,
                                          skyline::utils::g_MainTextAddr, skyline::utils::g_MainRodataAddr,
                                          skyline::utils::g_MainDataAddr, skyline::utils::g_MainBssAddr,
                                          skyline::utils::g_MainHeapAddr);
    skyline::logger::s_Instance->LogFormat("[skyline-totk] romfs mounted at '%s'",
                                          skyline::utils::g_RomMountStr.c_str());

    if (!totk::g_Config.load_plugins) {
        skyline::logger::s_Instance->Log("[skyline-totk] Plugin loading disabled by config.\n");
        return;
    }

    // Note: bypassing the singleton-like system because some games have issues
    // with the __cxa_guard_acquire gcc emits for function-local statics.
    auto manager = new skyline::plugin::Manager();
    manager->LoadPluginsImpl();
}

static skyline::utils::Task* after_romfs_task = nullptr;

void stub() {}

static skyline::utils::Once g_MountRomInit;
Result (*nnFsMountRomImpl)(char const*, void*, unsigned long);

Result handleNnFsMountRom(char const* path, void* buffer, unsigned long size) {
    Result rc = nnFsMountRomImpl(path, buffer, size);

    skyline::utils::g_RomMountStr = std::string(path) + ":/";

    // Some games call this several times, so only bring Skyline up once.
    g_MountRomInit.call_once([]() {
        after_romfs_task = new skyline::utils::Task{[]() { setup_after_romfs(); }};

        // Run the rest on a thread of our own: the calling thread's stack
        // belongs to the game, and plugin initialization is stack hungry (a
        // plugin may parse or decompress a few megabytes before returning).
        skyline::utils::SafeTaskQueue* taskQueue = new skyline::utils::SafeTaskQueue(100);
        if (!taskQueue->startThread(20, -1, 0x80000)) {
            // Never wait on an event nothing will ever signal: let the game boot.
            skyline::logger::s_Instance->Log("[skyline-totk] Worker thread failed to start, skipping init.\n");
            return;
        }
        taskQueue->push(new std::unique_ptr<skyline::utils::Task>(after_romfs_task));
        nn::os::WaitEvent(&after_romfs_task->completionEvent);
    });

    return rc;
}

static skyline::utils::Once g_RoInit;
Result (*nnRoInitializeImpl)();

Result nn_ro_init() {
    Result ret = 0;

    g_RoInit.call_once([&ret]() {
        ret = nnRoInitializeImpl();
        skyline::logger::s_Instance->LogFormat("[skyline-totk] Ran hooked nn::ro::Initialize (0x%x)", ret);
    });

    return ret;
}

void skyline_main() {
    // populate our own process handle
    envSetOwnProcessHandle(skyline::proc_handle::Get());

    // Logging before the heap is up goes straight to the kernel.
    skyline::logger::s_Instance = &g_BootLogger;

    // init hooking setup (maps its trampoline pools through the JIT svcs, no heap)
    A64HookInit();

    // override exception handler to dump info
    nn::os::SetUserExceptionHandler(exception_handler, exception_handler_stack, sizeof(exception_handler_stack),
                                    &exception_info);

    // Everything else is deferred to this hook: it is the first point where the
    // game's romfs, filesystem and allocator are all known to be up.
    A64HookFunction(reinterpret_cast<void*>(nn::fs::MountRom), reinterpret_cast<void*>(handleNnFsMountRom),
                    (void**)&nnFsMountRomImpl);

    A64HookFunction(reinterpret_cast<void*>(nn::ro::Initialize), reinterpret_cast<void*>(nn_ro_init),
                    (void**)&nnRoInitializeImpl);

    skyline::logger::s_Instance->Log("[skyline-totk] Module initialized, waiting for romfs.\n");
}

extern "C" void skyline_init() {
    skyline::utils::init();
    virtmemSetup();  // needed for libnx JIT

    skyline_main();
}
