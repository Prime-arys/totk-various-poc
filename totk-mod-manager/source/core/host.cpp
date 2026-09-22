// The tkm_host_* functions the Rust core calls for file access (it has no
// standard library of its own on the Switch). Paths come resolved: "sdmc:/..."
// for the SD card, "game:/..." for the game's romfs, both through newlib and
// libnx's devoptabs. Directory listings on the SD card use libnx directly,
// which hands out file sizes without a stat per file.

#include <dirent.h>
#include <sys/stat.h>
#include <unistd.h>

#include <cerrno>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <mutex>
#include <string>

#include <switch.h>

#include "util/log.hpp"

namespace
{

struct ReadHandle
{
    FILE* file;
    std::mutex lock;
    char* buffer;
};

struct WriteHandle
{
    FILE* file;
    char* buffer;
};

struct HostFile
{
    bool writing;
    union {
        ReadHandle* reader;
        WriteHandle* writer;
    };
};

constexpr size_t READ_BUFFER  = 256 * 1024;
constexpr size_t WRITE_BUFFER = 1024 * 1024;

struct DirHandle
{
    // SD card: native directory with sizes.
    bool native = false;
    FsDir dir{};
    FsDirectoryEntry entries[64];
    s64 count = 0;
    s64 index = 0;
    // Anything else: newlib.
    DIR* posix = nullptr;
    std::string path;
};

/** "sdmc:/totk/mods" -> "/totk/mods", when the path is on the SD card. */
bool sdmcRelative(const char* path, std::string& out)
{
    static const char prefix[] = "sdmc:/";
    if (std::strncmp(path, prefix, sizeof(prefix) - 1) != 0)
        return false;
    out = std::string("/") + (path + sizeof(prefix) - 1);
    if (out.size() > 1 && out.back() == '/')
        out.pop_back();
    return true;
}

} // namespace

extern "C"
{

int tkm_host_stat(const char* path, int* is_dir, uint64_t* len)
{
    struct stat info;
    if (stat(path, &info) != 0)
        return errno ? errno : -1;
    *is_dir = S_ISDIR(info.st_mode) ? 1 : 0;
    *len    = S_ISDIR(info.st_mode) ? 0 : (uint64_t)info.st_size;
    return 0;
}

void* tkm_host_open_read(const char* path, uint64_t* len)
{
    FILE* file = fopen(path, "rb");
    if (!file)
        return nullptr;
    if (fseeko(file, 0, SEEK_END) != 0)
    {
        fclose(file);
        return nullptr;
    }
    *len = (uint64_t)ftello(file);
    fseeko(file, 0, SEEK_SET);

    auto* reader   = new ReadHandle();
    reader->file   = file;
    reader->buffer = (char*)malloc(READ_BUFFER);
    if (reader->buffer)
        setvbuf(file, reader->buffer, _IOFBF, READ_BUFFER);

    auto* handle    = new HostFile();
    handle->writing = false;
    handle->reader  = reader;
    return handle;
}

int64_t tkm_host_read_at(void* opaque, uint64_t offset, uint8_t* buffer, size_t len)
{
    auto* handle = (HostFile*)opaque;
    if (!handle || handle->writing)
        return -1;
    ReadHandle* reader = handle->reader;
    std::lock_guard<std::mutex> guard(reader->lock);
    if (fseeko(reader->file, (off_t)offset, SEEK_SET) != 0)
        return -2;
    size_t done = 0;
    while (done < len)
    {
        size_t read = fread(buffer + done, 1, len - done, reader->file);
        if (read == 0)
            break;
        done += read;
    }
    if (done < len && ferror(reader->file))
        return -3;
    return (int64_t)done;
}

void* tkm_host_open_write(const char* path)
{
    FILE* file = fopen(path, "wb");
    if (!file)
        return nullptr;
    auto* writer   = new WriteHandle();
    writer->file   = file;
    writer->buffer = (char*)malloc(WRITE_BUFFER);
    if (writer->buffer)
        setvbuf(file, writer->buffer, _IOFBF, WRITE_BUFFER);

    auto* handle    = new HostFile();
    handle->writing = true;
    handle->writer  = writer;
    return handle;
}

int64_t tkm_host_write(void* opaque, const uint8_t* data, size_t len)
{
    auto* handle = (HostFile*)opaque;
    if (!handle || !handle->writing)
        return -1;
    return (int64_t)fwrite(data, 1, len, handle->writer->file);
}

int tkm_host_close(void* opaque)
{
    auto* handle = (HostFile*)opaque;
    if (!handle)
        return 0;
    int result = 0;
    if (handle->writing)
    {
        result = fclose(handle->writer->file) == 0 ? 0 : (errno ? errno : -1);
        free(handle->writer->buffer);
        delete handle->writer;
    }
    else
    {
        fclose(handle->reader->file);
        free(handle->reader->buffer);
        delete handle->reader;
    }
    delete handle;
    return result;
}

void* tkm_host_dir_open(const char* path)
{
    auto* handle = new DirHandle();
    std::string relative;
    FsFileSystem* sdmc = fsdevGetDeviceFileSystem("sdmc");
    if (sdmc && sdmcRelative(path, relative))
    {
        char native[FS_MAX_PATH];
        std::snprintf(native, sizeof(native), "%s", relative.c_str());
        if (R_SUCCEEDED(fsFsOpenDirectory(sdmc, native, FsDirOpenMode_ReadDirs | FsDirOpenMode_ReadFiles, &handle->dir)))
        {
            handle->native = true;
            return handle;
        }
        delete handle;
        return nullptr;
    }

    handle->posix = opendir(path);
    if (!handle->posix)
    {
        delete handle;
        return nullptr;
    }
    handle->path = path;
    if (!handle->path.empty() && handle->path.back() != '/')
        handle->path.push_back('/');
    return handle;
}

int tkm_host_dir_next(void* opaque, uint8_t* name, size_t capacity, int* is_dir, uint64_t* len)
{
    auto* handle = (DirHandle*)opaque;
    if (!handle || capacity == 0)
        return -1;

    if (handle->native)
    {
        if (handle->index >= handle->count)
        {
            handle->index = 0;
            if (R_FAILED(fsDirRead(&handle->dir, &handle->count, 64, handle->entries)) || handle->count <= 0)
                return 0;
        }
        const FsDirectoryEntry& entry = handle->entries[handle->index++];
        std::snprintf((char*)name, capacity, "%s", entry.name);
        *is_dir = entry.type == FsDirEntryType_Dir ? 1 : 0;
        *len    = *is_dir ? 0 : (uint64_t)entry.file_size;
        return 1;
    }

    struct dirent* entry = readdir(handle->posix);
    if (!entry)
        return 0;
    std::snprintf((char*)name, capacity, "%s", entry->d_name);
    *len = 0;
    if (entry->d_type == DT_DIR)
    {
        *is_dir = 1;
    }
    else
    {
        *is_dir = 0;
        struct stat info;
        std::string full = handle->path + entry->d_name;
        if (stat(full.c_str(), &info) == 0)
        {
            *is_dir = S_ISDIR(info.st_mode) ? 1 : 0;
            *len    = *is_dir ? 0 : (uint64_t)info.st_size;
        }
    }
    return 1;
}

void tkm_host_dir_close(void* opaque)
{
    auto* handle = (DirHandle*)opaque;
    if (!handle)
        return;
    if (handle->native)
        fsDirClose(&handle->dir);
    if (handle->posix)
        closedir(handle->posix);
    delete handle;
}

int tkm_host_mkdir(const char* path)
{
    return mkdir(path, 0777) == 0 ? 0 : (errno ? errno : -1);
}

int tkm_host_remove_file(const char* path)
{
    return unlink(path) == 0 ? 0 : (errno ? errno : -1);
}

int tkm_host_remove_dir(const char* path)
{
    return rmdir(path) == 0 ? 0 : (errno ? errno : -1);
}

int tkm_host_rename(const char* from, const char* to)
{
    return rename(from, to) == 0 ? 0 : (errno ? errno : -1);
}

uint64_t tkm_host_ticks_us(void)
{
    return armTicksToNs(armGetSystemTick()) / 1000;
}

[[noreturn]] void tkm_host_panic(const uint8_t* message, size_t length)
{
    std::string text((const char*)message, length);
    applog::write("core panic: " + text);
    applog::flush();
    diagAbortWithResult(MAKERESULT(Module_HomebrewAbi, 0x100));
}

} // extern "C"
