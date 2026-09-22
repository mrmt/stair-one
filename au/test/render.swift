// インストールした AU を AVAudioEngine のオフライン描画で鳴らし、音が出るか確かめる
//   swift au/test/render.swift [パッド 1-16]
import AVFoundation

let pad = CommandLine.arguments.count > 1 ? Int(CommandLine.arguments[1])! : 1
let desc = AudioComponentDescription(componentType: kAudioUnitType_MusicDevice,
                                     componentSubType: FourCharCode("Str1"), componentManufacturer: FourCharCode("Mrmt"),
                                     componentFlags: 0, componentFlagsMask: 0)
extension FourCharCode {
    init(_ s: String) { self = s.utf8.reduce(0) { ($0 << 8) | FourCharCode($1) } }
}

let engine = AVAudioEngine()
let sem = DispatchSemaphore(value: 0)
var unit: AVAudioUnit?
AVAudioUnit.instantiate(with: desc, options: []) { u, err in
    if let err { print("instantiate failed:", err); exit(1) }
    unit = u; sem.signal()
}
sem.wait()
guard let au = unit else { exit(1) }
engine.attach(au)
let fmt = AVAudioFormat(standardFormatWithSampleRate: 48000, channels: 2)!
engine.connect(au, to: engine.mainMixerNode, format: fmt)
try! engine.enableManualRenderingMode(.offline, format: fmt, maximumFrameCount: 512)
try! engine.start()

let midi = au.auAudioUnit.scheduleMIDIEventBlock!
let note = UInt8(35 + pad)
let buf = AVAudioPCMBuffer(pcmFormat: engine.manualRenderingFormat, frameCapacity: 512)!
var peak: Float = 0, bad = 0, peakAfter: Float = 0
let total = 48000 * 4
var done = 0, off = false
while done < total {
    if done == 0 { midi(AUEventSampleTimeImmediate, 0, 3, [0x90, note, 100]) }
    if !off && done >= 48000 * 2 { midi(AUEventSampleTimeImmediate, 0, 3, [0x80, note, 0]); off = true }
    _ = try! engine.renderOffline(512, to: buf)
    for ch in 0..<2 {
        let p = buf.floatChannelData![ch]
        for i in 0..<Int(buf.frameLength) {
            let v = p[i]
            if !v.isFinite { bad += 1 }
            if done < 48000 * 2 { peak = max(peak, abs(v)) } else if done > 48000 * 3 + 24000 { peakAfter = max(peakAfter, abs(v)) }
        }
    }
    done += 512
}
print(String(format: "pad %d  peak %.3f  after release %.4f  non-finite %d", pad, peak, peakAfter, bad))
// 離して 1.5 秒後にはディレイの残りだけになっている
exit(peak > 0.01 && bad == 0 && peakAfter < peak / 5 ? 0 : 1)
