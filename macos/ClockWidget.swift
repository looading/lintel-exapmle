import AppIntents
import SwiftUI
import WidgetKit

struct CityConfiguration: WidgetConfigurationIntent {
    static let title: LocalizedStringResource = "选择城市"

    @Parameter(title: "首选城市", default: .local)
    var primary: ClockCity

    @Parameter(title: "第二城市（中号小组件）", default: .london)
    var secondary: ClockCity
}

struct ClockEntry: TimelineEntry {
    let date: Date
    let configuration: CityConfiguration
}

struct CityProvider: AppIntentTimelineProvider {
    func placeholder(in context: Context) -> ClockEntry {
        ClockEntry(date: .now, configuration: CityConfiguration())
    }

    func snapshot(for configuration: CityConfiguration, in context: Context) async -> ClockEntry {
        ClockEntry(date: .now, configuration: configuration)
    }

    func timeline(for configuration: CityConfiguration, in context: Context) async -> Timeline<ClockEntry> {
        let now = Date.now
        let nextMinute = Date(timeIntervalSinceReferenceDate:
            (floor(now.timeIntervalSinceReferenceDate / 60) + 1) * 60
        )
        let entries = [ClockEntry(date: now, configuration: configuration)] + (0..<60).map { minute in
            ClockEntry(date: nextMinute.addingTimeInterval(Double(minute) * 60), configuration: configuration)
        }
        return Timeline(entries: entries, policy: .atEnd)
    }
}

struct CityClockPanel: View {
    let city: ClockCity
    let date: Date

    var body: some View {
        VStack(alignment: .leading, spacing: 5) {
            Text(String(localized: city.name))
                .font(.subheadline.weight(.semibold))
                .foregroundStyle(.secondary)
                .lineLimit(1)
            // Resolve the city's time zone before WidgetKit archives the text.
            Text(verbatim: date.formatted(city.clockFormat))
                .font(.system(size: 32, weight: .medium, design: .monospaced))
                .minimumScaleFactor(0.62)
                .lineLimit(1)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}

struct ClockWidgetView: View {
    @Environment(\.widgetFamily) private var family
    let entry: ClockEntry

    var body: some View {
        VStack(alignment: .leading, spacing: 15) {
            Label("世界时钟", systemImage: "globe.asia.australia")
                .font(.caption.weight(.semibold))
                .foregroundStyle(.secondary)
            HStack(spacing: 13) {
                CityClockPanel(city: entry.configuration.primary, date: entry.date)
                if family == .systemMedium {
                    Divider()
                    CityClockPanel(city: entry.configuration.secondary, date: entry.date)
                }
            }
            .frame(maxHeight: .infinity)
        }
        .containerBackground(for: .widget) {
            Color(red: 0.86, green: 0.92, blue: 0.89)
        }
        .foregroundStyle(Color(red: 0.09, green: 0.22, blue: 0.23))
        .widgetURL(URL(string: "lintel-world-clock://world"))
    }
}

@main
struct ClockWidget: Widget {
    var body: some WidgetConfiguration {
        AppIntentConfiguration(kind: "world-city-clock", intent: CityConfiguration.self, provider: CityProvider()) {
            ClockWidgetView(entry: $0)
        }
        .configurationDisplayName("城市时间")
        .description("在桌面或通知中心查看所选城市的当地时间。")
        .supportedFamilies([.systemSmall, .systemMedium])
    }
}
