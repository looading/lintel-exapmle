import AppIntents
import AppKit
import ExtensionFoundation
import Foundation

@main
struct ClockIntents: AppIntentsExtension {}

struct CityTimeIntent: AppIntent {
    static let title: LocalizedStringResource = "查看城市时间"
    static let description = IntentDescription("返回指定城市当前的当地时间。")

    @Parameter(title: "城市", default: .local)
    var city: ClockCity

    static var parameterSummary: some ParameterSummary {
        Summary("查看\(\.$city)时间")
    }

    func perform() async throws -> some IntentResult & ReturnsValue<String> & ProvidesDialog {
        let time = Date().formatted(city.clockFormat)
        let reply = "\(String(localized: city.name))现在是 \(time)"
        return .result(value: time, dialog: IntentDialog(stringLiteral: reply))
    }
}

struct StartTimerIntent: AppIntent {
    static let title: LocalizedStringResource = "开始世界时钟计时器"
    static let description = IntentDescription("用世界时钟新建倒计时，时长以秒为单位。")

    @Parameter(title: "秒数", default: 300)
    var seconds: Int

    @Parameter(title: "名称")
    var label: String?

    static var parameterSummary: some ParameterSummary {
        Summary("开始 \(\.$seconds) 秒的计时器") {
            \.$label
        }
    }

    func perform() async throws -> some IntentResult & ProvidesDialog {
        guard (1...359_999).contains(seconds) else { throw ClockIntentError.invalidDuration }
        try await openClock(action: "timer", key: "seconds", value: String(seconds), label: label)
        return .result(dialog: "请在世界时钟中确认计时请求。")
    }
}

struct SetAlarmIntent: AppIntent {
    static let title: LocalizedStringResource = "设置世界时钟闹钟"
    static let description = IntentDescription("用本地时间设置单次闹钟。")

    @Parameter(title: "小时（0–23）", default: 7)
    var hour: Int

    @Parameter(title: "分钟（0–59）", default: 30)
    var minute: Int

    @Parameter(title: "名称")
    var label: String?

    static var parameterSummary: some ParameterSummary {
        Summary("设置 \(\.$hour) 时 \(\.$minute) 分的闹钟") {
            \.$label
        }
    }

    func perform() async throws -> some IntentResult & ProvidesDialog {
        guard (0...23).contains(hour), (0...59).contains(minute) else { throw ClockIntentError.invalidTime }
        let time = String(format: "%02d:%02d", hour, minute)
        try await openClock(action: "alarm", key: "time", value: time, label: label)
        return .result(dialog: "请在世界时钟中确认闹钟请求。")
    }
}

private enum ClockIntentError: LocalizedError {
    case invalidDuration
    case invalidTime
    case invalidLabel
    case invalidLink
    case hostNotFound

    var errorDescription: String? {
        switch self {
        case .invalidDuration: "计时器时长须大于等于 1 秒且小于 100 小时。"
        case .invalidTime: "小时须在 0 到 23 之间，分钟须在 0 到 59 之间。"
        case .invalidLabel: "名称最多 80 个字符，且不能包含控制字符。"
        case .invalidLink: "无法生成世界时钟操作链接。"
        case .hostNotFound: "找不到世界时钟应用。"
        }
    }
}

@MainActor
private func openClock(action: String, key: String, value: String, label: String?) async throws {
    let label = label ?? ""
    guard label.unicodeScalars.count <= 80,
          label.unicodeScalars.allSatisfy({ !CharacterSet.controlCharacters.contains($0) }) else {
        throw ClockIntentError.invalidLabel
    }
    var link = URLComponents()
    link.scheme = "lintel-world-clock"
    link.host = action
    link.queryItems = [URLQueryItem(name: key, value: value), URLQueryItem(name: "label", value: label)]
    // Keep literal plus signs distinct from encoded spaces in URL form parsing.
    link.percentEncodedQuery = link.percentEncodedQuery?.replacingOccurrences(of: "+", with: "%2B")
    guard let url = link.url else { throw ClockIntentError.invalidLink }

    let host = Bundle.main.bundleURL
        .deletingLastPathComponent()
        .deletingLastPathComponent()
        .deletingLastPathComponent()
    guard host.pathExtension == "app" else {
        throw ClockIntentError.hostNotFound
    }
    let configuration = NSWorkspace.OpenConfiguration()
    configuration.allowsRunningApplicationSubstitution = false
    _ = try await NSWorkspace.shared.open([url], withApplicationAt: host, configuration: configuration)
}
