#include "skyline/logger/TcpLogger.hpp"

#include <atomic>

#include "skyline/utils/cpputils.hpp"

#define POLLIN 0x01

extern "C" void skyline_tcp_send_raw(char* data, size_t size) __attribute__((visibility("default")));

// Plugin-facing entrypoint: anything a plugin logs goes through the active logger.
void skyline_tcp_send_raw(char* data, u64 size) { skyline::logger::s_Instance->Log(data, size); }

namespace skyline::logger {

int g_tcpSocket = -1;
u16 g_tcpPort = 6969;
std::atomic<bool> g_socketInit{false};

static Result stub() { return 0; };

static void listen_thread_main(void*) {
    struct sockaddr_in serverAddr;
    s32 listenSocket = nn::socket::Socket(AF_INET, SOCK_STREAM, 0);
    if (listenSocket < 0) return;

    int flags = 1;
    nn::socket::SetSockOpt(listenSocket, SOL_SOCKET, SO_KEEPALIVE, &flags, sizeof(flags));

    serverAddr.sin_family = AF_INET;
    serverAddr.sin_addr.s_addr = INADDR_ANY;
    serverAddr.sin_port = nn::socket::InetHtons(g_tcpPort);

    if (nn::socket::Bind(listenSocket, (struct sockaddr*)&serverAddr, sizeof(serverAddr)) < 0) {
        nn::socket::Close(listenSocket);
        return;
    }

    if (nn::socket::Listen(listenSocket, 1) < 0) {
        nn::socket::Close(listenSocket);
        return;
    }

    // Poll with a 1-second timeout so the thread can exit cleanly on emulators.
    s32 clientSocket = -1;
    while (true) {
        nn::socket::PollFd pfd;
        pfd.fd = listenSocket;
        pfd.events = POLLIN;
        pfd.revents = 0;

        s32 pollResult = nn::socket::Poll(&pfd, 1, 1000);
        if (pollResult > 0 && (pfd.revents & POLLIN)) {
            u32 addrLen = sizeof(serverAddr);
            clientSocket = nn::socket::Accept(listenSocket, (struct sockaddr*)&serverAddr, &addrLen);
            break;
        }
    }

    nn::socket::Close(listenSocket);

    if (clientSocket < 0) return;
    g_tcpSocket = clientSocket;

    const char* message = "TCP Socket Connected.\n";
    nn::socket::Send(g_tcpSocket, (void*)message, strlen(message), 0);
}

void skyline_socket_init() {
    if (g_socketInit.load(std::memory_order_acquire)) return;

    // Sized for Skyline's log socket plus a plugin or two talking to the
    // network (e.g. one fetching a mod pack); the game gets to keep the rest.
    const size_t poolSize = 0x400000;
    void* socketPool = memalign(0x4000, poolSize);
    nn::socket::Initialize(socketPool, poolSize, 0x20000, 14);

    g_socketInit.store(true, std::memory_order_release);
}

void start_listen_thread() {
    const size_t stackSize = 0x4000;
    void* threadStack = memalign(0x1000, stackSize);

    nn::os::ThreadType* thread = new nn::os::ThreadType;
    if (R_FAILED(nn::os::CreateThread(thread, listen_thread_main, nullptr, threadStack, stackSize, 16))) return;
    nn::os::StartThread(thread);
}

static Result init_normal(void*, ulong, ulong, int) { return 0; }

static Result init_config(nn::socket::Config const&) { return 0; }

void setup_socket_hooks() {
    static bool installed = false;
    if (installed) return;
    installed = true;

    // The socket library can only be initialized once per process: since we got
    // there first, keep the game from trying (or from tearing it back down).
    Result (*socketInitWithPool)(void*, ulong, ulong, int) = nn::socket::Initialize;
    A64HookFunction(reinterpret_cast<void*>(socketInitWithPool), reinterpret_cast<void*>(init_normal), NULL);

    Result (*socketInitWithConfig)(nn::socket::Config const&) = nn::socket::Initialize;
    A64HookFunction(reinterpret_cast<void*>(socketInitWithConfig), reinterpret_cast<void*>(init_config), NULL);

    A64HookFunction(reinterpret_cast<void*>(nn::socket::Finalize), reinterpret_cast<void*>(stub), NULL);
}

void TcpLogger::Initialize() {}

};  // namespace skyline::logger

// Plugin-facing: brings up nn::socket once for the whole process and keeps the
// game from initializing (or finalizing) it again. A plugin that needs the
// network before the game has started it — to download a mod pack, say — must
// go through this rather than calling nn::socket::Initialize itself, or the
// game aborts later when its own initialization fails.
extern "C" bool skyline_totk_init_sockets() {
    skyline::logger::skyline_socket_init();
    skyline::logger::setup_socket_hooks();
    return skyline::logger::g_socketInit.load(std::memory_order_acquire);
}

namespace skyline::logger {

bool TcpLogger::ShouldFlush() { return true; }

void TcpLogger::SendRaw(void* data, size_t size) {
    if (g_socketInit.load(std::memory_order_acquire) && g_tcpSocket != -1)
        nn::socket::Send(g_tcpSocket, data, size, 0);
}

};  // namespace skyline::logger
