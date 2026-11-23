import AppIntents
import Foundation

enum ClockCity: String, AppEnum {
    case local, shanghai, tokyo, seoul, taipei, hongKong, singapore, jakarta
    case london, paris, dubai, newYork, losAngeles, sydney

    static let typeDisplayRepresentation: TypeDisplayRepresentation = "城市"

    static let caseDisplayRepresentations: [Self: DisplayRepresentation] = [
        .local: "本地", .shanghai: "上海", .tokyo: "东京", .seoul: "首尔",
        .taipei: "台北", .hongKong: "香港", .singapore: "新加坡", .jakarta: "雅加达",
        .london: "伦敦", .paris: "巴黎", .dubai: "迪拜", .newYork: "纽约",
        .losAngeles: "洛杉矶", .sydney: "悉尼"
    ]

    var name: LocalizedStringResource {
        Self.caseDisplayRepresentations[self]?.title ?? "本地"
    }

    var timeZone: TimeZone {
        let id: String
        switch self {
        case .local: return .autoupdatingCurrent
        case .shanghai: id = "Asia/Shanghai"
        case .tokyo: id = "Asia/Tokyo"
        case .seoul: id = "Asia/Seoul"
        case .taipei: id = "Asia/Taipei"
        case .hongKong: id = "Asia/Hong_Kong"
        case .singapore: id = "Asia/Singapore"
        case .jakarta: id = "Asia/Jakarta"
        case .london: id = "Europe/London"
        case .paris: id = "Europe/Paris"
        case .dubai: id = "Asia/Dubai"
        case .newYork: id = "America/New_York"
        case .losAngeles: id = "America/Los_Angeles"
        case .sydney: id = "Australia/Sydney"
        }
        return TimeZone(identifier: id) ?? .gmt
    }

    var clockFormat: Date.VerbatimFormatStyle {
        Date.VerbatimFormatStyle(
            format: "\(hour: .twoDigits(clock: .twentyFourHour, hourCycle: .zeroBased)):\(minute: .twoDigits)",
            locale: Locale(identifier: "zh_CN"),
            timeZone: timeZone,
            calendar: Calendar(identifier: .gregorian)
        )
    }
}
