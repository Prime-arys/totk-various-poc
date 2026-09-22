#include "util/log.hpp"

#include <unistd.h>

#include <cstdio>
#include <ctime>
#include <mutex>

#include <borealis/core/logger.hpp>
#include <switch.h>

namespace applog
{

namespace
{
    std::mutex lock;
    FILE* file = nullptr;
}

void open(const std::string& path)
{
    std::lock_guard<std::mutex> guard(lock);
    if (file)
        fclose(file);
    file = fopen(path.c_str(), "w");
}

void write(const std::string& line)
{
    // Shows up in emulator logs and debuggers.
    svcOutputDebugString(line.c_str(), line.size());
    std::lock_guard<std::mutex> guard(lock);
    if (!file)
        return;
    std::time_t now = std::time(nullptr);
    char stamp[16];
    std::strftime(stamp, sizeof(stamp), "%H:%M:%S", std::localtime(&now));
    std::fprintf(file, "[%s] %s\n", stamp, line.c_str());
    std::fflush(file);
    // Down to the SD card: the log is what is left to read after a crash.
    fsync(fileno(file));
}

void flush()
{
    std::lock_guard<std::mutex> guard(lock);
    if (file)
        std::fflush(file);
}

} // namespace applog
