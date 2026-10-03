import AppKit
import AVFoundation
import Carbon.HIToolbox
import os
import Speech

/// What voice does, stage by stage, in the system log (`log show --predicate
/// 'subsystem == "dev.hover.desktop" AND category == "voice"'`). Never the words heard.
let voiceLog = Logger(subsystem: "dev.hover.desktop", category: "voice")

// Speech to text on this Mac, live: the microphone through AVAudioEngine, words as they
// are said. macOS 26 and later use SpeechAnalyzer with SpeechTranscriber (the model
// behind Notes and Voice Memos: on device, punctuated, good at long and distant speech);
// earlier systems use SFSpeechRecognizer, on device where the language allows. Nothing
// is recorded to disk and no audio leaves the Mac, unless the system's own recognizer
// has no on-device model for the language (macOS 14/15 only), which Settings says.

struct DictationError: LocalizedError {
    enum Kind { case microphone, speech, unsupported, engine }
    let kind: Kind
    let message: String
    var errorDescription: String? { message }
    /// Where in System Settings the user can fix it.
    var settingsURL: URL? {
        switch kind {
        case .microphone: return URL(string: "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone")
        case .speech: return URL(string: "x-apple.systempreferences:com.apple.preference.security?Privacy_SpeechRecognition")
        default: return nil
        }
    }
}

/// What a dictation engine reports, always on the main thread.
protocol DictationSink: AnyObject {
    func dictationLevel(_ level: Float)
    func dictationText(final: String, partial: String)
    func dictationPreparing(_ what: String?)
}

enum Permissions {
    static var microphone: AVAuthorizationStatus { AVCaptureDevice.authorizationStatus(for: .audio) }
    static var speech: SFSpeechRecognizerAuthorizationStatus { SFSpeechRecognizer.authorizationStatus() }

    /// Asks for what is still undecided; throws for what was refused.
    static func ensure() async throws {
        switch microphone {
        case .authorized: break
        case .notDetermined:
            let ok = await withCheckedContinuation { c in AVCaptureDevice.requestAccess(for: .audio) { c.resume(returning: $0) } }
            if !ok { throw DictationError(kind: .microphone, message: "Hover can’t hear you: microphone access is off. Turn on Hover in Privacy & Security → Microphone.") }
        default: throw DictationError(kind: .microphone, message: "Hover can’t hear you: microphone access is off. Turn on Hover in Privacy & Security → Microphone.")
        }
        switch speech {
        case .authorized: break
        case .notDetermined:
            let s = await withCheckedContinuation { c in SFSpeechRecognizer.requestAuthorization { c.resume(returning: $0) } }
            if s != .authorized { throw DictationError(kind: .speech, message: "Speech recognition is off for Hover. Turn it on in Privacy & Security → Speech Recognition.") }
        default: throw DictationError(kind: .speech, message: "Speech recognition is off for Hover. Turn it on in Privacy & Security → Speech Recognition.")
        }
    }
}

/// Loudness of a buffer as 0…1, on a speech-friendly scale (−55 dBFS is silence).
func audioLevel(_ buffer: AVAudioPCMBuffer) -> Float {
    let n = Int(buffer.frameLength)
    guard n > 0 else { return 0 }
    var sum: Float = 0
    if let f = buffer.floatChannelData?[0] { for i in 0..<n { sum += f[i] * f[i] } }
    else if let s = buffer.int16ChannelData?[0] { for i in 0..<n { let v = Float(s[i]) / 32768; sum += v * v } }
    else { return 0 }
    let db = 20 * log10(max(1e-7, sqrt(sum / Float(n))))
    return min(1, max(0, (db + 55) / 45))
}

/// Runs work, giving up on waiting for it after `seconds`.
func withTimeout(_ seconds: Double, _ work: @escaping @Sendable () async -> Void) async {
    await withTaskGroup(of: Void.self) { group in
        group.addTask { await work() }
        group.addTask { try? await Task.sleep(nanoseconds: UInt64(seconds * 1e9)) }
        await group.next(); group.cancelAll()
    }
}

/// One live dictation, start to finish. Each start makes a new one.
final class Dictation {
    weak var sink: DictationSink?
    private var impl: DictationImpl?
    private(set) var engineName = ""

    /// Which engine this Mac uses, for Settings.
    static var engineDescription: String {
        #if compiler(>=6.2)
        if #available(macOS 26.0, *) { return "Apple’s on-device speech model (SpeechAnalyzer)" }
        #endif
        return "macOS dictation (SFSpeechRecognizer, on device when the language allows)"
    }

    func start(locale: Locale) async throws {
        voiceLog.info("start: microphone \(Permissions.microphone.rawValue, privacy: .public), speech \(Permissions.speech.rawValue, privacy: .public)")
        try await Permissions.ensure()
        voiceLog.info("start: permissions granted")
        let impl: DictationImpl
        #if compiler(>=6.2)
        if #available(macOS 26.0, *) { impl = AnalyzerDictation() } else { impl = LegacyDictation() }
        #else
        impl = LegacyDictation()
        #endif
        impl.sink = sink
        self.impl = impl
        try await impl.start(locale: locale)
        voiceLog.info("start: listening")
    }

    /// Stops listening and returns everything heard (waits for the last words).
    func finish() async -> String { await impl?.finish() ?? "" }

    func cancel() { impl?.cancel(); impl = nil }
}

private protocol DictationImpl: AnyObject {
    var sink: DictationSink? { get set }
    func start(locale: Locale) async throws
    func finish() async -> String
    func cancel()
}

/// The text heard so far: settled sentences, and the words still being guessed.
private final class Heard {
    private let lock = NSLock()
    private var settled = "", guess = ""
    func set(final f: String? = nil, partial p: String) -> (String, String) {
        lock.lock(); defer { lock.unlock() }
        if let f { settled = f }
        guess = p
        return (settled, guess)
    }
    func append(final f: String) -> (String, String) {
        lock.lock(); defer { lock.unlock() }
        settled = join(settled, f); guess = ""
        return (settled, guess)
    }
    var all: String { lock.lock(); defer { lock.unlock() }; return join(settled, guess).trimmingCharacters(in: .whitespacesAndNewlines) }
    private func join(_ a: String, _ b: String) -> String {
        let b = b.trimmingCharacters(in: .whitespaces)
        if a.isEmpty { return b }
        if b.isEmpty { return a }
        return a + (a.hasSuffix(" ") ? "" : " ") + b
    }
}

// MARK: macOS 26+: SpeechAnalyzer

// The macOS 26 SDK (Xcode 26, Swift 6.2) has SpeechAnalyzer; an older one builds only the
// recognizer below.
#if compiler(>=6.2)
@available(macOS 26.0, *)
final class AnalyzerDictation: DictationImpl {
    weak var sink: DictationSink?
    private let engine = AVAudioEngine()
    private var analyzer: SpeechAnalyzer?
    private var input: AsyncStream<AnalyzerInput>.Continuation?
    private var results: Task<Void, Never>?
    private let heard = Heard()
    private var tapped = false

    func start(locale: Locale) async throws {
        let mic = engine.inputNode.outputFormat(forBus: 0)
        guard mic.sampleRate > 0, mic.channelCount > 0 else { throw DictationError(kind: .engine, message: "No microphone is available.") }
        let feed = try await prepare(locale: locale, from: mic)
        engine.inputNode.installTap(onBus: 0, bufferSize: 2048, format: mic) { [weak self] buffer, _ in
            let level = audioLevel(buffer)
            DispatchQueue.main.async { self?.sink?.dictationLevel(level) }
            feed(buffer)
        }
        tapped = true
        engine.prepare()
        do { try engine.start() } catch { throw DictationError(kind: .engine, message: "The microphone couldn’t start: \(error.localizedDescription)") }
    }

    /// Gets the model and the analyzer ready for audio in `source`'s format, and returns
    /// what takes each buffer (converted to the model's format) from then on.
    func prepare(locale: Locale, from source: AVAudioFormat) async throws -> (AVAudioPCMBuffer) -> Void {
        var found = await SpeechTranscriber.supportedLocale(equivalentTo: locale)
        if found == nil { found = await SpeechTranscriber.supportedLocale(equivalentTo: Locale(identifier: "en-US")) }
        guard let lang = found else {
            throw DictationError(kind: .unsupported, message: "Speech recognition doesn’t support \(locale.localizedString(forIdentifier: locale.identifier) ?? locale.identifier) on this Mac.")
        }
        let transcriber = SpeechTranscriber(locale: lang, transcriptionOptions: [], reportingOptions: [.volatileResults], attributeOptions: [])
        // The model is the system's, shared by every app; it is fetched once.
        if await AssetInventory.status(forModules: [transcriber]) != .installed {
            let name = lang.localizedString(forIdentifier: lang.identifier) ?? lang.identifier
            await MainActor.run { self.sink?.dictationPreparing("Getting the \(name) speech model ready…") }
            do {
                if let request = try await AssetInventory.assetInstallationRequest(supporting: [transcriber]) { try await request.downloadAndInstall() }
            } catch {
                throw DictationError(kind: .unsupported, message: "The speech model couldn’t be downloaded: \(error.localizedDescription)")
            }
            await MainActor.run { self.sink?.dictationPreparing(nil) }
        }
        let analyzer = SpeechAnalyzer(modules: [transcriber])
        self.analyzer = analyzer
        guard let format = await SpeechAnalyzer.bestAvailableAudioFormat(compatibleWith: [transcriber], considering: source) else {
            throw DictationError(kind: .engine, message: "The speech model can’t read this microphone’s audio.")
        }
        lastFormat = format
        try await analyzer.prepareToAnalyze(in: format)
        // The names the router listens for ("Ask Kiro to …", "Tell Pip …"), which the
        // model otherwise hears as other words ("Piro").
        let context = AnalysisContext()
        context.contextualStrings[.general] = ["Kiro", "Codex", "Cursor", "OpenCode", "Claude Code", "Hover"] + VoiceRoute.bots
        try? await analyzer.setContext(context)
        let (stream, continuation) = AsyncStream<AnalyzerInput>.makeStream(bufferingPolicy: .bufferingNewest(256))
        input = continuation
        let heard = self.heard
        results = Task {
            do {
                for try await r in transcriber.results {
                    let text = String(r.text.characters)
                    let (f, p) = r.isFinal ? heard.append(final: text) : heard.set(partial: text)
                    DispatchQueue.main.async { [weak self] in self?.sink?.dictationText(final: f, partial: p) }
                }
            } catch {}
        }
        try await analyzer.start(inputSequence: stream)
        let converter = source == format ? nil : AVAudioConverter(from: source, to: format)
        return { buffer in
            guard let out = converter.map({ Self.convert(buffer, with: $0, to: format) }) ?? buffer else { return }
            continuation.yield(AnalyzerInput(buffer: out))
        }
    }
    private(set) var lastFormat: AVAudioFormat?

    private static func convert(_ buffer: AVAudioPCMBuffer, with converter: AVAudioConverter, to format: AVAudioFormat) -> AVAudioPCMBuffer? {
        let ratio = format.sampleRate / buffer.format.sampleRate
        let capacity = AVAudioFrameCount((Double(buffer.frameLength) * ratio).rounded(.up)) + 16
        guard let out = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: capacity) else { return nil }
        var fed = false, error: NSError?
        let status = converter.convert(to: out, error: &error) { _, state in
            if fed { state.pointee = .noDataNow; return nil }
            fed = true; state.pointee = .haveData; return buffer
        }
        return status == .error || out.frameLength == 0 ? nil : out
    }

    private func stopMic() {
        if tapped { engine.inputNode.removeTap(onBus: 0); tapped = false }
        if engine.isRunning { engine.stop() }
        input?.finish(); input = nil
    }

    func finish() async -> String {
        stopMic()
        if let analyzer { try? await analyzer.finalizeAndFinishThroughEndOfInput() }
        // The last results arrive after finalising; the stream ends with them. A model
        // that never ends its stream still lets go after a moment.
        if let results { await withTimeout(3) { await results.value } }
        analyzer = nil
        return heard.all
    }

    func cancel() {
        stopMic()
        results?.cancel()
        if let analyzer { Task { await analyzer.cancelAndFinishNow() } }
        analyzer = nil
    }
}
#endif

// MARK: macOS 14–15: SFSpeechRecognizer

private final class LegacyDictation: DictationImpl {
    weak var sink: DictationSink?
    private let engine = AVAudioEngine()
    private var request: SFSpeechAudioBufferRecognitionRequest?
    private var task: SFSpeechRecognitionTask?
    private var done: CheckedContinuation<Void, Never>?
    private var ended = false
    private let heard = Heard()
    private var tapped = false

    func start(locale: Locale) async throws {
        guard let recognizer = SFSpeechRecognizer(locale: locale) ?? SFSpeechRecognizer(locale: Locale(identifier: "en-US")), recognizer.isAvailable else {
            throw DictationError(kind: .unsupported, message: "Speech recognition isn’t available right now.")
        }
        let request = SFSpeechAudioBufferRecognitionRequest()
        request.shouldReportPartialResults = true
        request.addsPunctuation = true
        request.taskHint = .dictation
        request.contextualStrings = ["Kiro", "Codex", "Cursor", "OpenCode", "Claude Code", "Hover"] + VoiceRoute.bots
        if recognizer.supportsOnDeviceRecognition { request.requiresOnDeviceRecognition = true }
        self.request = request
        let heard = self.heard
        task = recognizer.recognitionTask(with: request) { [weak self] result, error in
            if let result {
                let (f, p) = result.isFinal ? heard.set(final: result.bestTranscription.formattedString, partial: "") : heard.set(partial: result.bestTranscription.formattedString)
                DispatchQueue.main.async { self?.sink?.dictationText(final: f, partial: p) }
            }
            if result?.isFinal == true || error != nil { DispatchQueue.main.async { self?.end() } }
        }
        let mic = engine.inputNode.outputFormat(forBus: 0)
        guard mic.sampleRate > 0 else { throw DictationError(kind: .engine, message: "No microphone is available.") }
        engine.inputNode.installTap(onBus: 0, bufferSize: 2048, format: mic) { [weak self] buffer, _ in
            request.append(buffer)
            let level = audioLevel(buffer)
            DispatchQueue.main.async { self?.sink?.dictationLevel(level) }
        }
        tapped = true
        engine.prepare()
        do { try engine.start() } catch { throw DictationError(kind: .engine, message: "The microphone couldn’t start: \(error.localizedDescription)") }
    }

    private func end() { ended = true; done?.resume(); done = nil }

    private func stopMic() {
        if tapped { engine.inputNode.removeTap(onBus: 0); tapped = false }
        if engine.isRunning { engine.stop() }
    }

    func finish() async -> String {
        stopMic()
        request?.endAudio()
        // The recognizer's callbacks and this wait meet on the main queue.
        await withCheckedContinuation { (c: CheckedContinuation<Void, Never>) in
            DispatchQueue.main.async {
                if self.ended { c.resume(); return }
                self.done = c
                // A recognizer that never says final still lets go after a moment.
                DispatchQueue.main.asyncAfter(deadline: .now() + 2.5) { [weak self] in self?.end() }
            }
        }
        task?.finish()
        return heard.all
    }

    func cancel() { stopMic(); task?.cancel(); request = nil; DispatchQueue.main.async { self.end() } }
}

// MARK: A check of the whole pipeline, with no window and no prompts

/// `open -n -g -a Hover --args --voice-probe <report> [<audio file>]` writes what voice
/// needs and what it does to <report>, then quits: the permissions as they stand (never
/// asked for here), the microphone's format and loudness for a second when it is
/// allowed, and the words the speech model hears in the audio file. Runs as Hover, so
/// the grants checked are Hover's own.
enum VoiceProbe {
    static func run(report: String, audio: String?) async {
        var out: [String] = []
        func line(_ s: String) { out.append(s); try? out.joined(separator: "\n").appending("\n").write(toFile: report, atomically: true, encoding: .utf8) }
        line("macOS \(ProcessInfo.processInfo.operatingSystemVersionString)")
        line("engine: \(Dictation.engineDescription)")
        line("locale: \(Locale.autoupdatingCurrent.identifier)")
        let mic = Permissions.microphone, speech = Permissions.speech
        line("microphone: \(["notDetermined", "restricted", "denied", "authorized"][min(3, mic.rawValue)])")
        line("speech: \(["notDetermined", "denied", "restricted", "authorized"][min(3, speech.rawValue)])")
        // The shortcuts, registered as Hover registers them (Hover itself isn't running).
        for (name, code, mods) in [("Control-Option-Space", kVK_Space, controlKey | optionKey), ("Option-N", kVK_ANSI_N, optionKey)] {
            var ref: EventHotKeyRef?
            let r = RegisterEventHotKey(UInt32(code), UInt32(mods), EventHotKeyID(signature: OSType(0x48565250), id: 9), GetApplicationEventTarget(), 0, &ref)
            line("hotkey \(name): \(r == noErr ? "registered" : "failed \(r)")")
            if let ref { UnregisterEventHotKey(ref) }
        }
        // HOVER_PROBE_KEYS=<seconds>: Control-Option-Space held that long, counting its
        // presses and releases (and nothing else happens).
        if let s = ProcessInfo.processInfo.environment["HOVER_PROBE_KEYS"].flatMap(Double.init) {
            let seen = Peak()
            var specs = [EventTypeSpec(eventClass: OSType(kEventClassKeyboard), eventKind: UInt32(kEventHotKeyPressed)),
                         EventTypeSpec(eventClass: OSType(kEventClassKeyboard), eventKind: UInt32(kEventHotKeyReleased))]
            var handler: EventHandlerRef?, ref: EventHotKeyRef?
            InstallEventHandler(GetApplicationEventTarget(), { _, event, data in
                guard let event, let data else { return OSStatus(eventNotHandledErr) }
                Unmanaged<Peak>.fromOpaque(data).takeUnretainedValue().add(GetEventKind(event) == UInt32(kEventHotKeyPressed) ? 1 : 0.5)
                return noErr
            }, 2, &specs, Unmanaged.passUnretained(seen).toOpaque(), &handler)
            RegisterEventHotKey(UInt32(kVK_Space), UInt32(controlKey | optionKey), EventHotKeyID(signature: OSType(0x48565250), id: 8), GetApplicationEventTarget(), 0, &ref)
            line("waiting \(s) s for Control-Option-Space")
            try? await Task.sleep(nanoseconds: UInt64(s * 1e9))
            line("hotkey events: \(seen.count) (presses and releases)")
            if let ref { UnregisterEventHotKey(ref) }
            if let handler { RemoveEventHandler(handler) }
        }
        if mic == .authorized {
            let engine = AVAudioEngine()
            let format = engine.inputNode.outputFormat(forBus: 0)
            line("mic format: \(format)")
            let peak = Peak()
            engine.inputNode.installTap(onBus: 0, bufferSize: 2048, format: format) { b, _ in peak.add(audioLevel(b)) }
            do {
                try engine.start()
                try? await Task.sleep(nanoseconds: 1_200_000_000)
                engine.stop(); engine.inputNode.removeTap(onBus: 0)
                line("mic buffers: \(peak.count), peak level: \(String(format: "%.2f", peak.max))")
            } catch { line("mic start failed: \(error.localizedDescription)") }
        }
        if let audio {
            #if compiler(>=6.2)
            if #available(macOS 26.0, *) {
                do {
                    let file = try AVAudioFile(forReading: URL(fileURLWithPath: audio))
                    let d = AnalyzerDictation()
                    let started = Date()
                    let feed = try await d.prepare(locale: Locale.autoupdatingCurrent, from: file.processingFormat)
                    line("model ready in \(Int(Date().timeIntervalSince(started) * 1000)) ms, format \(d.lastFormat.map { "\($0)" } ?? "?")")
                    while file.framePosition < file.length {
                        guard let b = AVAudioPCMBuffer(pcmFormat: file.processingFormat, frameCapacity: 4096) else { break }
                        try file.read(into: b, frameCount: 4096)
                        if b.frameLength == 0 { break }
                        feed(b)
                    }
                    let text = await d.finish()
                    line("heard: \(text.isEmpty ? "(nothing)" : text)")
                    let (task, target) = VoiceRoute.agent(in: text, sessions: [])
                    if case .new(let tool)? = target { line("routed: new \(tool) task: \(task)") } else { line("routed: default agent, task: \(task)") }
                } catch { line("transcription failed: \(error.localizedDescription)") }
            } else { line("transcription: skipped (needs macOS 26)") }
            #else
            line("transcription: skipped (built without the macOS 26 SDK)")
            #endif
        }
        line("done")
    }

    private final class Peak: @unchecked Sendable {
        private let lock = NSLock()
        private(set) var count = 0, max: Float = 0
        func add(_ v: Float) { lock.lock(); count += 1; max = Swift.max(max, v); lock.unlock() }
    }
}
