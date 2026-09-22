#include "util/archive.hpp"

#include <sys/stat.h>

#include <algorithm>
#include <cerrno>
#include <cstdio>
#include <strings.h>
#include <vector>

#include <archive.h>
#include <archive_entry.h>

#include "util/log.hpp"

namespace unpack
{

namespace
{
    bool endsWith(const std::string& text, const char* suffix)
    {
        size_t length = std::char_traits<char>::length(suffix);
        return text.size() >= length && strcasecmp(text.c_str() + text.size() - length, suffix) == 0;
    }

    void makeDirectories(const std::string& path)
    {
        for (size_t slash = path.find('/', path.find(":/") + 2); slash != std::string::npos;
             slash        = path.find('/', slash + 1))
            mkdir(path.substr(0, slash).c_str(), 0777);
        mkdir(path.c_str(), 0777);
    }

    /** Archive entry names as safe relative paths: no "..", no leading "/". */
    std::string sanitize(const char* name)
    {
        std::string result;
        std::string part;
        auto flush = [&]() {
            if (part.empty() || part == "." || part == "..")
            {
                part.clear();
                return;
            }
            // FAT32 refuses these, and some archives made on Linux have them.
            for (char& c : part)
                if (c == ':' || c == '*' || c == '?' || c == '"' || c == '<' || c == '>' || c == '|')
                    c = '_';
            if (!result.empty())
                result.push_back('/');
            result += part;
            part.clear();
        };
        for (const char* c = name; *c; c++)
        {
            if (*c == '/' || *c == '\\')
                flush();
            else
                part.push_back(*c);
        }
        flush();
        return result;
    }

    struct archive* openArchive(const std::string& file)
    {
        struct archive* reader = archive_read_new();
        archive_read_support_format_all(reader);
        archive_read_support_filter_all(reader);
        if (archive_read_open_filename(reader, file.c_str(), 256 * 1024) != ARCHIVE_OK)
        {
            applog::write("libarchive: " + std::string(archive_error_string(reader) ? archive_error_string(reader) : "?"));
            archive_read_free(reader);
            return nullptr;
        }
        return reader;
    }
} // namespace

bool isArchive(const std::string& name)
{
    for (const char* extension : { ".zip", ".7z", ".rar", ".tar", ".gz", ".tgz", ".xz", ".bz2", ".zst" })
        if (endsWith(name, extension))
            return true;
    return false;
}

std::string extract(const std::string& file, const std::string& destination, Progress progress)
{
    // A first pass for the total size, so the progress bar means something.
    uint64_t total = 0;
    if (struct archive* reader = openArchive(file))
    {
        struct archive_entry* entry;
        while (archive_read_next_header(reader, &entry) == ARCHIVE_OK)
        {
            if (archive_entry_size_is_set(entry))
                total += (uint64_t)archive_entry_size(entry);
            archive_read_data_skip(reader);
        }
        archive_read_free(reader);
    }

    struct archive* reader = openArchive(file);
    if (!reader)
        return "unsupported or damaged archive";

    makeDirectories(destination);
    std::vector<char> buffer(512 * 1024);
    std::vector<char> fileBuffer(1024 * 1024);
    uint64_t done = 0;
    std::string error;

    struct archive_entry* entry;
    int status;
    while ((status = archive_read_next_header(reader, &entry)) == ARCHIVE_OK || status == ARCHIVE_WARN)
    {
        std::string relative = sanitize(archive_entry_pathname(entry));
        if (relative.empty())
            continue;
        std::string target = destination + "/" + relative;

        if (archive_entry_filetype(entry) == AE_IFDIR)
        {
            makeDirectories(target);
            continue;
        }
        if (archive_entry_filetype(entry) != AE_IFREG)
            continue;

        size_t slash = target.rfind('/');
        if (slash != std::string::npos)
            makeDirectories(target.substr(0, slash));

        FILE* out = fopen(target.c_str(), "wb");
        if (!out)
        {
            error = "cannot create " + target;
            break;
        }
        setvbuf(out, fileBuffer.data(), _IOFBF, fileBuffer.size());
        la_ssize_t read;
        while ((read = archive_read_data(reader, buffer.data(), buffer.size())) > 0)
        {
            if (fwrite(buffer.data(), 1, (size_t)read, out) != (size_t)read)
            {
                error = "could not write " + target + " (SD card full?)";
                break;
            }
            done += (uint64_t)read;
            if (progress && !progress(done, std::max(total, done)))
            {
                error = "cancelled";
                break;
            }
        }
        if (read < 0 && error.empty())
            error = archive_error_string(reader) ? archive_error_string(reader) : "damaged archive";
        if (fclose(out) != 0 && error.empty())
            error = "could not write " + target;
        if (!error.empty())
            break;
    }
    if (error.empty() && status != ARCHIVE_EOF)
        error = archive_error_string(reader) ? archive_error_string(reader) : "damaged archive";

    archive_read_free(reader);
    if (!error.empty())
        applog::write("extracting " + file + ": " + error);
    return error;
}

} // namespace unpack
