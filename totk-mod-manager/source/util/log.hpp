#pragma once

#include <string>

// The manager's log file (sd:/totk/manager.log): what the merge, downloads
// and installs did, for when something needs explaining.
namespace applog
{

void open(const std::string& path);
void write(const std::string& line);
void flush();

} // namespace applog
