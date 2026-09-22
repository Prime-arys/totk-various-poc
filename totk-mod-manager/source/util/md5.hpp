#pragma once

#include <string>

// MD5, to check downloads against GameBanana's checksums (as TKMM does).
namespace md5
{

/** Lowercase hex digest of a file, or an empty string if it cannot be read. */
std::string ofFile(const std::string& path);

std::string ofText(const std::string& text);

} // namespace md5
