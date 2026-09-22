#pragma once

#include "net/gamebanana.hpp"

// Downloading a GameBanana file and turning it into a mod folder, the way
// TKMM installs from GameBanana: a .tkcl as is, an archive unpacked and
// searched for .tkcl files or romfs/exefs folders.
namespace installer
{

void install(const gamebanana::Submission& submission, const gamebanana::File& file);

} // namespace installer
