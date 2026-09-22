#include "skyline/logger/SdLogger.hpp"

#include "nn/fs.h"

namespace skyline::logger {

namespace {
    // Creates every missing directory along "sd:/a/b/c/file.log".
    void createParentDirectories(const std::string& path) {
        size_t start = path.find(":/");
        if (start == std::string::npos) return;
        start += 2;

        size_t slash = path.find('/', start);
        while (slash != std::string::npos) {
            std::string dir = path.substr(0, slash);
            nn::fs::DirectoryEntryType type;
            if (R_FAILED(nn::fs::GetEntryType(&type, dir.c_str()))) nn::fs::CreateDirectory(dir.c_str());
            slash = path.find('/', slash + 1);
        }
    }
};  // namespace

SdLogger::SdLogger(std::string path) {
    createParentDirectories(path);

    nn::fs::DirectoryEntryType type;
    Result rc = nn::fs::GetEntryType(&type, path.c_str());

    if (R_FAILED(rc)) {
        // Most likely 0x202 (path not found): make the file and try again.
        if (R_FAILED(nn::fs::CreateFile(path.c_str(), 0))) return;
    } else if (type == nn::fs::DirectoryEntryType_Directory) {
        return;
    }

    // Start each boot with a fresh log rather than growing one forever.
    if (R_FAILED(nn::fs::OpenFile(&m_handle, path.c_str(), nn::fs::OpenMode_ReadWrite | nn::fs::OpenMode_Append)))
        return;

    nn::fs::SetFileSize(m_handle, 0);
    m_open = true;
}

void SdLogger::Initialize() {
    // nothing to do
}

bool SdLogger::ShouldFlush() { return false; }

void SdLogger::SendRaw(void* data, size_t size) {
    if (!m_open) return;

    nn::fs::SetFileSize(m_handle, m_offset + size);
    nn::fs::WriteFile(m_handle, m_offset, data, size, nn::fs::WriteOption::CreateOption(nn::fs::WriteOptionFlag_Flush));
    m_offset += size;
};

};  // namespace skyline::logger
