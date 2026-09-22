// tkmm-oracle: runs TKMM's own code (TkSharp) so the Rust merger can be checked
// against it.
//
//   tkmm-oracle package <romfs> <project folder> <out.tkcl>
//       Builds a .tkcl from a TKMM project folder (romfs/, options/<group>/<option>/...).
//
//   tkmm-oracle merge <romfs> <output folder> <input>...
//       Merges mods (.tkcl files or mod folders, lowest priority first) with the
//       default option selection, the way TKMM would, into <output folder>.
//
//   tkmm-oracle changelog <romfs> <input> <output folder>
//       Imports a single mod and copies the changelog files TKMM produced for it.

using TkSharp;
using TkSharp.Core;
using TkSharp.Core.Extensions;
using TkSharp.Core.IO;
using TkSharp.Core.IO.Caching;
using TkSharp.Core.Models;
using TkSharp.Data.Embedded;
using TkSharp.IO.Writers;
using TkSharp.Merging;
using TkSharp.Packaging;

if (args.Length < 3) {
    Console.Error.WriteLine("usage: tkmm-oracle package|merge|changelog <romfs> ...");
    return 2;
}

TkLog.Instance.Register(new ConsoleLog());

var command = args[0];
var romfs = Path.GetFullPath(args[1]);

var checksums = TkChecksums.FromStream(TkEmbeddedDataSource.GetChecksumsBin());
var packFileLookup = new TkPackFileLookup(TkEmbeddedDataSource.GetPackFileLookup());
using var rom = new ExtractedTkRom(romfs, checksums, packFileLookup);
Console.WriteLine($"rom: version {rom.GameVersion}, nso {rom.NsoBinaryId}");

// Static session state TKMM's changelog builders rely on (GameData delta
// cache). Next to the tool rather than in the temporary directory, which on
// Windows is whatever TMP/TEMP say — C:\WINDOWS when they say nothing, where
// this cannot write. TKMM_ORACLE_CACHE moves it elsewhere.
var cacheFolder = Environment.GetEnvironmentVariable("TKMM_ORACLE_CACHE") is { Length: > 0 } chosenCache
    ? chosenCache
    : Path.Combine(AppContext.BaseDirectory, "cache");
Directory.CreateDirectory(cacheFolder);
TkChangelogBuilder.Init(new RomProvider(rom), cacheFolder);

switch (command) {
    case "package": {
        var project = TkProjectManager.OpenProject(Path.GetFullPath(args[2]));
        await using var output = File.Create(args[3]);
        await project.Package(output, rom);
        Console.WriteLine($"packaged {args[3]}");
        return 0;
    }

    case "merge": {
        var outputFolder = Path.GetFullPath(args[2]);
        if (Directory.Exists(outputFolder)) {
            Directory.Delete(outputFolder, recursive: true);
        }

        var dataFolder = Path.Combine(Path.GetTempPath(), $"tkmm-oracle-{Guid.NewGuid():N}");
        var manager = TkModManager.Create(dataFolder);
        var readers = new TkModReaderProvider(manager, new RomProvider(rom));

        List<TkChangelog> changelogs = [];
        foreach (var input in args.Skip(3)) {
            object source = File.Exists(input) ? Path.GetFullPath(input) : Path.GetFullPath(input);
            if (await readers.ReadFromInput(source) is not TkMod mod) {
                Console.Error.WriteLine($"could not read {input}");
                return 1;
            }

            manager.Import(mod);
            changelogs.Add(mod.Changelog);
            foreach (var group in mod.OptionGroups) {
                foreach (var option in group.DefaultSelectedOptions) {
                    changelogs.Add(option.Changelog);
                }
            }
            Console.WriteLine($"imported {mod.Name} ({mod.Changelog.ChangelogFiles.Count} changelog entries)");
        }

        var merger = new TkMerger(new FolderModWriter(outputFolder), rom);
        merger.Merge(changelogs);
        Console.WriteLine($"merged into {outputFolder}");
        return 0;
    }

    case "changelog": {
        var outputFolder = Path.GetFullPath(args[3]);
        var dataFolder = Path.Combine(Path.GetTempPath(), $"tkmm-oracle-{Guid.NewGuid():N}");
        var manager = TkModManager.Create(dataFolder);
        var readers = new TkModReaderProvider(manager, new RomProvider(rom));

        if (await readers.ReadFromInput(Path.GetFullPath(args[2])) is not TkMod mod) {
            Console.Error.WriteLine($"could not read {args[2]}");
            return 1;
        }

        manager.Import(mod);
        foreach (var entry in mod.Changelog.ChangelogFiles) {
            Console.WriteLine($"{entry.Type,-11} {entry.Canonical} attrs={entry.Attributes} dict={entry.ZsDictionaryId} " +
                              $"versions=[{string.Join(",", entry.Versions)}] archives=[{string.Join(",", entry.ArchiveCanonicals)}]");
        }

        var modFolder = Path.Combine(dataFolder, "contents", mod.Id.ToString());
        if (Directory.Exists(modFolder)) {
            CopyDirectory(modFolder, outputFolder);
            Console.WriteLine($"changelog files copied to {outputFolder}");
        }
        return 0;
    }

    default:
        Console.Error.WriteLine($"unknown command {command}");
        return 2;
}

static void CopyDirectory(string source, string destination)
{
    foreach (var file in Directory.EnumerateFiles(source, "*", SearchOption.AllDirectories)) {
        var target = Path.Combine(destination, Path.GetRelativePath(source, file));
        Directory.CreateDirectory(Path.GetDirectoryName(target)!);
        File.Copy(file, target, overwrite: true);
    }
}

sealed class RomProvider(ITkRom rom) : ITkRomProvider
{
    public ITkRom GetRom() => rom;
}

sealed class ConsoleLog : Microsoft.Extensions.Logging.ILogger
{
    public IDisposable? BeginScope<TState>(TState state) where TState : notnull => null;

    public bool IsEnabled(Microsoft.Extensions.Logging.LogLevel logLevel) => logLevel >= Microsoft.Extensions.Logging.LogLevel.Information;

    public void Log<TState>(Microsoft.Extensions.Logging.LogLevel logLevel, Microsoft.Extensions.Logging.EventId eventId, TState state,
        Exception? exception, Func<TState, Exception?, string> formatter)
    {
        Console.WriteLine($"[{logLevel}] {formatter(state, exception)}");
        if (exception is not null) {
            Console.WriteLine(exception);
        }
    }
}
