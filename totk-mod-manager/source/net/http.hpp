#pragma once

#include <atomic>
#include <cstdint>
#include <functional>
#include <string>
#include <vector>

// HTTPS through libcurl, which devkitPro builds on the console's own TLS
// service (system certificates, nothing to bundle).
namespace http
{

struct Response
{
    long status = 0;
    std::string body;
    std::string error; // empty when the transfer itself worked
    bool ok() const { return error.empty() && status >= 200 && status < 300; }
};

/** Called with bytes received and expected (0 when unknown); return false
 *  to cancel. */
using Progress = std::function<bool(uint64_t done, uint64_t total)>;

void init();
/** Makes transfers in progress, and any started later, fail at once: the
 *  manager is closing. */
void abort();
void cleanup();

Response get(const std::string& url, long timeoutSeconds = 30);

/** Downloads to a file (replaced). Returns an empty string on success. */
std::string download(const std::string& url, const std::string& path, Progress progress);

std::string escape(const std::string& text);

} // namespace http
