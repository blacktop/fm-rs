// Foundation Models 27 session APIs shared by all models.
// Selected by build.rs when the build SDK is 27.0 or newer;
// session_27_fallback.swift preserves the C ABI on older SDKs.

import CoreGraphics
import Foundation
import FoundationModels
import ImageIO

// The cross-import overlays are probed rather than assumed: the iOS
// simulator SDK ships only the CoreSpotlight overlay, not the Vision one.
// canImport only tests for the overlay; the public frameworks are imported.
#if canImport(_CoreSpotlight_FoundationModels)
    import CoreSpotlight
#endif
#if canImport(_Vision_FoundationModels)
    import Vision
#endif

private let foundationModels27Requirement =
    "This API requires macOS 27.0, iOS 27.0, visionOS 27.0, or later"

/// Maps Foundation Models 27 error types onto stable FFI codes, covering the
/// general `LanguageModelError` plus the split `SystemLanguageModel.Error`
/// and `LanguageModelSession.Error` cases.
func classifyLanguageModelError(_ error: Error) -> (FFIErrorCode, String)? {
    guard #available(iOS 27.0, macOS 27.0, visionOS 27.0, *) else {
        return nil
    }

    if let systemError = error as? SystemLanguageModel.Error {
        switch systemError {
        case .assetsUnavailable(let details):
            return (
                .assetsUnavailable,
                systemError.errorDescription ?? details.debugDescription
            )
        @unknown default:
            return (.generationFailed, systemError.localizedDescription)
        }
    }

    if let sessionError = error as? LanguageModelSession.Error {
        let message = sessionError.errorDescription ?? sessionError.debugDescription
        switch sessionError {
        case .concurrentRequests:
            return (.concurrentRequests, message)
        case .transcriptMutationWhileResponding:
            return (.invalidInput, message)
        @unknown default:
            return (.generationFailed, sessionError.localizedDescription)
        }
    }

    if let modelError = error as? LanguageModelError {
        let base = modelError.errorDescription
        switch modelError {
        case .contextSizeExceeded(let details):
            let message = base ?? details.debugDescription
            return (
                .contextSizeExceeded,
                message + " (context size \(details.contextSize) tokens, request \(details.tokenCount) tokens)"
            )
        case .rateLimited(let details):
            var message = base ?? details.debugDescription
            if let resetDate = details.resetDate {
                message += " (resets \(ISO8601DateFormatter().string(from: resetDate)))"
            }
            return (.rateLimited, message)
        case .guardrailViolation(let details):
            return (.guardrailViolation, base ?? details.debugDescription)
        case .refusal(let details):
            return (.refusal, base ?? details.debugDescription)
        case .unsupportedCapability(let details):
            return (.unsupportedCapability, base ?? details.debugDescription)
        case .unsupportedTranscriptContent(let details):
            return (.unsupportedTranscriptContent, base ?? details.debugDescription)
        case .unsupportedGenerationGuide(let details):
            return (.unsupportedGenerationGuide, base ?? details.debugDescription)
        case .unsupportedLanguageOrLocale(let details):
            return (.unsupportedLanguageOrLocale, base ?? details.debugDescription)
        case .timeout(let details):
            return (.timeout, base ?? details.debugDescription)
        @unknown default:
            return (.generationFailed, modelError.localizedDescription)
        }
    }

    return nil
}

// MARK: - Built-in System Tools

/// Instantiates Apple's built-in tools from the Vision and Core Spotlight
/// cross-import overlays. Returns nil and sets `errorOut` on failure.
func makeBuiltinTools(
    _ names: [String],
    errorOut: UnsafeMutablePointer<UnsafeMutableRawPointer?>?
) -> [any Tool]? {
    guard !names.isEmpty else { return [] }
    guard #available(iOS 27.0, macOS 27.0, visionOS 27.0, *) else {
        errorOut?.pointee = createError(
            "Built-in system tools require macOS 27.0, iOS 27.0, visionOS 27.0, or later",
            code: .unsupportedPlatform
        )
        return nil
    }

    var tools: [any Tool] = []
    for name in names {
        switch name {
        case "ocr":
            #if canImport(_Vision_FoundationModels)
                tools.append(OCRTool())
            #else
                errorOut?.pointee = createError(
                    "The OCR tool is not available in this platform SDK",
                    code: .unsupportedPlatform
                )
                return nil
            #endif
        case "barcodeReader":
            #if canImport(_Vision_FoundationModels)
                tools.append(BarcodeReaderTool())
            #else
                errorOut?.pointee = createError(
                    "The barcode reader tool is not available in this platform SDK",
                    code: .unsupportedPlatform
                )
                return nil
            #endif
        case "spotlightSearch":
            #if canImport(_CoreSpotlight_FoundationModels)
                tools.append(SpotlightSearchTool())
            #else
                errorOut?.pointee = createError(
                    "The Spotlight search tool is not available in this platform SDK",
                    code: .unsupportedPlatform
                )
                return nil
            #endif
        default:
            errorOut?.pointee = createError(
                "Unknown built-in tool '\(name)'",
                code: .invalidInput
            )
            return nil
        }
    }
    return tools
}

// MARK: - Session Usage

private struct SessionUsageDTO: Encodable {
    let inputTokens: Int
    let cachedInputTokens: Int
    let outputTokens: Int
    let reasoningTokens: Int
}

@available(iOS 27.0, macOS 27.0, visionOS 27.0, *)
private func usageJson(_ usage: LanguageModelSession.Usage) -> String? {
    let dto = SessionUsageDTO(
        inputTokens: usage.input.totalTokenCount,
        cachedInputTokens: usage.input.cachedTokenCount,
        outputTokens: usage.output.totalTokenCount,
        reasoningTokens: usage.output.reasoningTokenCount
    )
    guard let data = try? JSONEncoder().encode(dto) else { return nil }
    return String(data: data, encoding: .utf8)
}

/// Captures a response's exact token usage as JSON (Foundation Models 27).
func responseUsageJson<Content>(_ response: LanguageModelSession.Response<Content>) -> String? {
    guard #available(iOS 27.0, macOS 27.0, visionOS 27.0, *) else {
        return nil
    }
    return usageJson(response.usage)
}

/// Returns cumulative session token usage as JSON (Foundation Models 27).
@_cdecl("fm_session_usage")
public func fm_session_usage(
    _ sessionPtr: UnsafeMutableRawPointer,
    _ errorOut: UnsafeMutablePointer<UnsafeMutableRawPointer?>?
) -> UnsafeMutablePointer<CChar>? {
    guard #available(iOS 27.0, macOS 27.0, visionOS 27.0, *) else {
        errorOut?.pointee = createError(foundationModels27Requirement, code: .unsupportedPlatform)
        return nil
    }

    let state = Unmanaged<AnyObject>.fromOpaque(sessionPtr).takeUnretainedValue() as! SessionState
    guard let json = usageJson(state.session.usage) else {
        errorOut?.pointee = createError("Failed to encode session usage", code: .unknown)
        return nil
    }
    return strdup(json)
}

// MARK: - Transcript Controls

/// Replaces the session transcript (Foundation Models 27).
@_cdecl("fm_session_set_transcript")
public func fm_session_set_transcript(
    _ sessionPtr: UnsafeMutableRawPointer,
    _ transcriptJson: UnsafePointer<CChar>,
    _ errorOut: UnsafeMutablePointer<UnsafeMutableRawPointer?>?
) -> Bool {
    guard #available(iOS 27.0, macOS 27.0, visionOS 27.0, *) else {
        errorOut?.pointee = createError(foundationModels27Requirement, code: .unsupportedPlatform)
        return false
    }

    let state = Unmanaged<AnyObject>.fromOpaque(sessionPtr).takeUnretainedValue() as! SessionState
    guard !state.session.isResponding else {
        errorOut?.pointee = createError(
            "Cannot replace the transcript while the session is responding",
            code: .invalidInput
        )
        return false
    }

    let jsonString = String(cString: transcriptJson)
    guard let jsonData = jsonString.data(using: .utf8) else {
        errorOut?.pointee = createError("Transcript JSON is not valid UTF-8", code: .invalidInput)
        return false
    }

    do {
        let transcript = try JSONDecoder().decode(Transcript.self, from: jsonData)
        state.session.transcript = transcript
        return true
    } catch {
        errorOut?.pointee = createError(
            "Failed to decode transcript: \(error.localizedDescription)",
            code: .invalidInput
        )
        return false
    }
}

/// Sets or clears the transcript error handling policy (Foundation Models 27).
/// `policy`: 0 = clear, 1 = revert transcript, 2 = preserve transcript.
@_cdecl("fm_session_set_transcript_error_policy")
public func fm_session_set_transcript_error_policy(
    _ sessionPtr: UnsafeMutableRawPointer,
    _ policy: Int32,
    _ errorOut: UnsafeMutablePointer<UnsafeMutableRawPointer?>?
) -> Bool {
    guard #available(iOS 27.0, macOS 27.0, visionOS 27.0, *) else {
        errorOut?.pointee = createError(foundationModels27Requirement, code: .unsupportedPlatform)
        return false
    }

    let state = Unmanaged<AnyObject>.fromOpaque(sessionPtr).takeUnretainedValue() as! SessionState
    switch policy {
    case 0:
        state.session.transcriptErrorHandlingPolicy = nil
    case 1:
        state.session.transcriptErrorHandlingPolicy = .revertTranscript
    case 2:
        state.session.transcriptErrorHandlingPolicy = .preserveTranscript
    default:
        errorOut?.pointee = createError(
            "Transcript error policy must be 0 (clear), 1 (revert), or 2 (preserve)",
            code: .invalidInput
        )
        return false
    }
    return true
}

// MARK: - Multimodal Attachments

private struct AttachmentDTO: Decodable {
    let kind: String
    let path: String?
    let bufferIndex: Int?
    let label: String?
}

private struct PreparedImageAttachment: Sendable {
    enum Source: Sendable {
        case file(URL)
        case data(Data)
    }

    let source: Source
    let label: String?
}

private struct AttachmentInputError: Error, LocalizedError {
    let message: String
    var errorDescription: String? { message }
}

private func prepareImageAttachments(
    from dtos: [AttachmentDTO],
    buffers: UnsafePointer<UnsafePointer<UInt8>?>?,
    bufferLens: UnsafePointer<UInt>?,
    bufferCount: Int32
) throws -> [PreparedImageAttachment] {
    var inputs: [PreparedImageAttachment] = []
    for dto in dtos {
        let source: PreparedImageAttachment.Source
        switch dto.kind {
        case "file":
            guard let path = dto.path else {
                throw AttachmentInputError(message: "File attachment is missing a path")
            }
            source = .file(URL(fileURLWithPath: path))
        case "data":
            guard let index = dto.bufferIndex,
                  index >= 0,
                  index < Int(bufferCount),
                  let buffers,
                  let bufferLens,
                  let base = buffers[index]
            else {
                throw AttachmentInputError(message: "Data attachment references an invalid buffer")
            }
            let data = Data(bytes: base, count: Int(bufferLens[index]))
            guard let imageSource = CGImageSourceCreateWithData(data as CFData, nil),
                  CGImageSourceGetCount(imageSource) > 0
            else {
                throw AttachmentInputError(message: "Data attachment is not a decodable image")
            }
            source = .data(data)
        default:
            throw AttachmentInputError(message: "Attachment kind must be \"file\" or \"data\"")
        }
        inputs.append(PreparedImageAttachment(source: source, label: dto.label))
    }
    return inputs
}

@available(iOS 27.0, macOS 27.0, visionOS 27.0, *)
private func makeImageAttachments(
    from inputs: [PreparedImageAttachment]
) throws -> [Attachment<ImageAttachmentContent>] {
    var attachments: [Attachment<ImageAttachmentContent>] = []
    for input in inputs {
        var attachment: Attachment<ImageAttachmentContent>
        switch input.source {
        case .file(let url):
            attachment = Attachment(imageURL: url)
        case .data(let data):
            guard let source = CGImageSourceCreateWithData(data as CFData, nil),
                  let cgImage = CGImageSourceCreateImageAtIndex(source, 0, nil)
            else {
                throw AttachmentInputError(message: "Data attachment is not a decodable image")
            }
            attachment = Attachment(cgImage)
        }
        if let label = input.label {
            attachment = attachment.label(label)
        }
        attachments.append(attachment)
    }
    return attachments
}

/// Sends a prompt with image attachments and blocks until the response is
/// ready (Foundation Models 27 multimodal prompting).
@_cdecl("fm_session_respond_with_attachments")
public func fm_session_respond_with_attachments(
    _ sessionPtr: UnsafeMutableRawPointer,
    _ prompt: UnsafePointer<CChar>,
    _ optionsJson: UnsafePointer<CChar>?,
    _ attachmentsJson: UnsafePointer<CChar>,
    _ buffers: UnsafePointer<UnsafePointer<UInt8>?>?,
    _ bufferLens: UnsafePointer<UInt>?,
    _ bufferCount: Int32,
    _ errorOut: UnsafeMutablePointer<UnsafeMutableRawPointer?>?
) -> UnsafeMutablePointer<CChar>? {
    guard #available(iOS 27.0, macOS 27.0, visionOS 27.0, *) else {
        errorOut?.pointee = createError(foundationModels27Requirement, code: .unsupportedPlatform)
        return nil
    }

    let state = Unmanaged<AnyObject>.fromOpaque(sessionPtr).takeUnretainedValue() as! SessionState
    let promptString = String(cString: prompt)
    let options = parseGenerationOptions(optionsJson)

    let attachmentsString = String(cString: attachmentsJson)
    guard let attachmentsData = attachmentsString.data(using: .utf8),
          let dtos = try? JSONDecoder().decode([AttachmentDTO].self, from: attachmentsData)
    else {
        errorOut?.pointee = createError("Invalid attachments JSON", code: .invalidInput)
        return nil
    }

    let attachmentInputs: [PreparedImageAttachment]
    do {
        attachmentInputs = try prepareImageAttachments(
            from: dtos,
            buffers: buffers,
            bufferLens: bufferLens,
            bufferCount: bufferCount
        )
    } catch {
        errorOut?.pointee = createError(error.localizedDescription, code: .invalidInput)
        return nil
    }

    do {
        let (content, usageJson) = try AsyncWaiter.wait(session: state) {
            let attachments = try makeImageAttachments(from: attachmentInputs)
            let response = try await state.session.respond(options: options) {
                promptString
                for attachment in attachments {
                    attachment
                }
            }
            return (response.content, responseUsageJson(response))
        }
        state.setLastResponseUsage(usageJson)
        return strdup(content)
    } catch {
        errorOut?.pointee = createGenerationErrorFromException(error)
        return nil
    }
}
