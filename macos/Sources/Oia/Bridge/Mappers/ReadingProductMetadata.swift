// SPDX-License-Identifier: GPL-3.0-or-later

import Foundation

/// A file-backed product classification and price snapshot validated by Rust.
/// Swift only decodes it and formats the price for the current locale.
struct ReadingProductMetadata: Decodable, Equatable, Sendable {
    let version: UInt8
    let price: String?
    let currency: String?
    let priceIsFrom: Bool
    let capturedAt: String?

    private enum CodingKeys: String, CodingKey {
        case version, price, currency
        case priceIsFrom = "price_is_from"
        case capturedAt = "captured_at"
    }

    init(from decoder: Decoder) throws {
        let values = try decoder.container(keyedBy: CodingKeys.self)
        version = try values.decode(UInt8.self, forKey: .version)
        price = try values.decodeIfPresent(String.self, forKey: .price)
        currency = try values.decodeIfPresent(String.self, forKey: .currency)
        priceIsFrom = try values.decodeIfPresent(Bool.self, forKey: .priceIsFrom) ?? false
        capturedAt = try values.decodeIfPresent(String.self, forKey: .capturedAt)
    }

    static func decode(_ json: String?) -> Self? {
        guard let data = json?.data(using: .utf8),
              let product = try? JSONDecoder().decode(Self.self, from: data),
              product.version == 1
        else { return nil }
        return product
    }

    func formattedPrice(locale: Locale = .current) -> String? {
        guard let price, let currency,
              let amount = Decimal(string: price, locale: Locale(identifier: "en_US_POSIX"))
        else { return nil }
        let fractionDigits = price.split(separator: ".").dropFirst().first?.count ?? 0
        let formatted = amount.formatted(
            .currency(code: currency).locale(locale)
                .precision(.fractionLength(0 ... max(2, fractionDigits)))
        )
        return priceIsFrom ? "From \(formatted)" : formatted
    }
}
