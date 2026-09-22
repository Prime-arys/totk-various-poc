#pragma once

#include <cstdint>
#include <functional>
#include <string>

// Unpacking downloaded mods (zip, 7z, rar...) with libarchive.
namespace unpack
{

/** Bytes written so far and in total; return false to stop. */
using Progress = std::function<bool(uint64_t done, uint64_t total)>;

/** Whether a file name looks like something libarchive can open. */
bool isArchive(const std::string& name);

/** Extracts `file` into `destination`. Returns an empty string on success. */
std::string extract(const std::string& file, const std::string& destination, Progress progress);

} // namespace unpack
