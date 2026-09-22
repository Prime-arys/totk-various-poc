#include "totk/config.hpp"

#include <cstdlib>
#include <cstring>

#include "nn/fs.h"
#include "skyline/logger/Logger.hpp"

namespace totk {

Config g_Config;

namespace {

    std::string trim(const std::string& str) {
        size_t begin = str.find_first_not_of(" \t\r\n");
        if (begin == std::string::npos) return "";
        size_t end = str.find_last_not_of(" \t\r\n");
        return str.substr(begin, end - begin + 1);
    }

    bool parseBool(const std::string& value, bool fallback) {
        if (value == "1" || value == "true" || value == "yes" || value == "on") return true;
        if (value == "0" || value == "false" || value == "no" || value == "off") return false;
        return fallback;
    }

    u32 parseSinks(const std::string& value) {
        u32 sinks = Config::LogNone;
        size_t pos = 0;
        while (pos <= value.size()) {
            size_t next = value.find(',', pos);
            std::string token = trim(value.substr(pos, next == std::string::npos ? next : next - pos));
            if (token == "kernel" || token == "all") sinks |= Config::LogKernel;
            if (token == "sd" || token == "all") sinks |= Config::LogSd;
            if (token == "tcp" || token == "all") sinks |= Config::LogTcp;
            if (next == std::string::npos) break;
            pos = next + 1;
        }
        return sinks;
    }

    void applyEntry(const std::string& key, const std::string& value) {
        if (key == "log")
            g_Config.log_sinks = parseSinks(value);
        else if (key == "log_path")
            g_Config.log_path = value;
        else if (key == "tcp_port")
            g_Config.tcp_port = (u16)strtoul(value.c_str(), nullptr, 0);
        else if (key == "plugins")
            g_Config.load_plugins = parseBool(value, g_Config.load_plugins);
        else if (key == "plugins_dir")
            g_Config.plugins_dir = value;
    }

};  // namespace

void LoadConfig() {
    nn::fs::FileHandle handle;
    if (R_FAILED(nn::fs::OpenFile(&handle, Config::Path, nn::fs::OpenMode_Read))) return;

    s64 size = 0;
    if (R_FAILED(nn::fs::GetFileSize(&size, handle)) || size <= 0) {
        nn::fs::CloseFile(handle);
        return;
    }

    char buffer[0x1000];
    size_t toRead = (size_t)size < sizeof(buffer) - 1 ? (size_t)size : sizeof(buffer) - 1;
    Result rc = nn::fs::ReadFile(handle, 0, buffer, toRead);
    nn::fs::CloseFile(handle);
    if (R_FAILED(rc)) return;
    buffer[toRead] = '\0';

    std::string contents(buffer);
    size_t pos = 0;
    while (pos <= contents.size()) {
        size_t eol = contents.find('\n', pos);
        std::string line = trim(contents.substr(pos, eol == std::string::npos ? eol : eol - pos));
        if (eol == std::string::npos)
            pos = contents.size() + 1;
        else
            pos = eol + 1;

        if (line.empty() || line[0] == '#' || line[0] == ';' || line[0] == '[') continue;

        size_t equals = line.find('=');
        if (equals == std::string::npos) continue;

        applyEntry(trim(line.substr(0, equals)), trim(line.substr(equals + 1)));
    }
}

};  // namespace totk
