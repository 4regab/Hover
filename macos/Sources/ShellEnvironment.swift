import Foundation

// A Finder- or login-item-launched app gets launchd's minimal PATH, so kiro-cli,
// codex, cursor-agent and opencode (Homebrew, npm, ~/.local/bin, nvm, bun...) look
// missing. T3 Code's desktop app solves this by reading the variables from the
// user's own login shell (`$SHELL -ilc`, values between markers, 5 s timeout), then
// launchctl's PATH, merged ahead of what was inherited. Hover does the same, once,
// before the backend starts (pingdotgg/t3code packages/shared/src/shell.ts, MIT).
enum ShellEnvironment {
    static let names = ["PATH", "SSH_AUTH_SOCK", "HOMEBREW_PREFIX", "HOMEBREW_CELLAR", "HOMEBREW_REPOSITORY",
                        "XDG_CONFIG_HOME", "XDG_DATA_HOME", "LANG", "LC_ALL", "LC_CTYPE", "NVM_DIR", "BUN_INSTALL"]

    /// The environment the backend and the agents it starts should see.
    static func resolve(base: [String: String]) -> [String: String] {
        var env = base
        var shellValues: [String: String] = [:]
        for shell in candidates(base["SHELL"]) {
            if let values = read(shell: shell, names: names), !values.isEmpty { shellValues = values; break }
        }
        for (name, value) in shellValues where name != "PATH" && env[name]?.isEmpty != false { env[name] = value }
        let home = FileManager.default.homeDirectoryForCurrentUser.path
        // Common install locations last, so a GUI launch still finds tools if the shell probe fails.
        let known = ["/opt/homebrew/bin", "/opt/homebrew/sbin", "/usr/local/bin", home + "/.local/bin", home + "/.bun/bin",
                     home + "/.npm-global/bin", home + "/.cargo/bin", home + "/.opencode/bin", "/usr/bin", "/bin", "/usr/sbin", "/sbin"]
        env["PATH"] = merge([shellValues["PATH"], launchctlPath(), base["PATH"], known.joined(separator: ":")])
        // Agents print UTF-8; without a locale some CLIs fall back to ASCII.
        if env["LANG"] == nil && env["LC_ALL"] == nil && env["LC_CTYPE"] == nil { env["LC_CTYPE"] = "en_US.UTF-8" }
        return env
    }

    static func candidates(_ shell: String?) -> [String] {
        var seen = Set<String>(), out: [String] = []
        for c in [shell, userShell(), "/bin/zsh"] {
            guard let c = c?.trimmingCharacters(in: .whitespaces), !c.isEmpty, c.hasPrefix("/"), !seen.contains(c),
                  FileManager.default.isExecutableFile(atPath: c) else { continue }
            seen.insert(c); out.append(c)
        }
        return out
    }

    /// The login shell from the user record, for launches where SHELL isn't set.
    private static func userShell() -> String? {
        guard let pw = getpwuid(getuid()), let shell = pw.pointee.pw_shell else { return nil }
        return String(cString: shell)
    }

    static func command(for names: [String]) -> String {
        names.filter { $0.range(of: "^[A-Z0-9_]+$", options: .regularExpression) != nil }.map {
            "printf '%s\\n' '__HOVER_ENV_\($0)_START__'; printenv \($0) || true; printf '%s\\n' '__HOVER_ENV_\($0)_END__'"
        }.joined(separator: "; ")
    }

    static func extract(_ output: String, names: [String]) -> [String: String] {
        var values: [String: String] = [:]
        for name in names {
            guard let start = output.range(of: "__HOVER_ENV_\(name)_START__\n"),
                  let end = output.range(of: "\n__HOVER_ENV_\(name)_END__", range: start.upperBound..<output.endIndex) else {
                // An empty value prints START then END with nothing between.
                continue
            }
            let value = String(output[start.upperBound..<end.lowerBound])
            if !value.isEmpty { values[name] = value }
        }
        return values
    }

    static func read(shell: String, names: [String], timeout: TimeInterval = 5) -> [String: String]? {
        guard let output = run(shell, ["-ilc", command(for: names)], timeout: timeout) else { return nil }
        return extract(output, names: names)
    }

    private static func launchctlPath() -> String? {
        run("/bin/launchctl", ["getenv", "PATH"], timeout: 2)?.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    /// Runs with no stdin (an interactive rc that prompts gets EOF) and kills on timeout.
    private static func run(_ executable: String, _ arguments: [String], timeout: TimeInterval) -> String? {
        let process = Process(), out = Pipe()
        process.executableURL = URL(fileURLWithPath: executable)
        process.arguments = arguments
        process.standardInput = FileHandle.nullDevice
        process.standardOutput = out
        process.standardError = FileHandle.nullDevice
        var data = Data()
        let reading = DispatchGroup(); reading.enter()
        DispatchQueue.global(qos: .userInitiated).async { data = out.fileHandleForReading.readDataToEndOfFile(); reading.leave() }
        let done = DispatchSemaphore(value: 0)
        process.terminationHandler = { _ in done.signal() }
        do { try process.run() } catch { try? out.fileHandleForWriting.close(); return nil }
        try? out.fileHandleForWriting.close()
        if done.wait(timeout: .now() + timeout) == .timedOut {
            process.terminate()
            // A shell that ignores SIGTERM must not hold Hover's startup.
            if done.wait(timeout: .now() + 1) == .timedOut { kill(process.processIdentifier, SIGKILL) }
            _ = reading.wait(timeout: .now() + 1)
            return nil
        }
        _ = reading.wait(timeout: .now() + 1)
        return String(data: data, encoding: .utf8)
    }

    static func merge(_ values: [String?]) -> String {
        var seen = Set<String>(), out: [String] = []
        for value in values.compactMap({ $0 }) {
            for entry in value.split(separator: ":").map({ $0.trimmingCharacters(in: .whitespaces) }) where !entry.isEmpty && !seen.contains(entry) {
                seen.insert(entry); out.append(entry)
            }
        }
        return out.joined(separator: ":")
    }
}
