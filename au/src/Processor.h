// Stair One の AU / VST3 / Standalone。音は engine/ffi (Rust staticlib) が作り、ここはホストとの橋渡しだけ
#pragma once

#include <juce_audio_processors/juce_audio_processors.h>
#include <array>
#include <atomic>

#include "stair_ffi.h"

class StairProcessor : public juce::AudioProcessor
{
public:
    static constexpr int kPads = 16;
    // ホストの MIDI ノート 36-51 (Logic 表記 C1-D#2) をパッド 1-16 に割り当てる
    static constexpr int kFirstNote = 36;

    StairProcessor();
    ~StairProcessor() override;

    void prepareToPlay(double sampleRate, int blockSize) override;
    void releaseResources() override;
    bool isBusesLayoutSupported(const BusesLayout&) const override;
    void processBlock(juce::AudioBuffer<float>&, juce::MidiBuffer&) override;

    juce::AudioProcessorEditor* createEditor() override;
    bool hasEditor() const override { return true; }

    const juce::String getName() const override { return JucePlugin_Name; }
    bool acceptsMidi() const override { return true; }
    bool producesMidi() const override { return false; }
    // ディレイのフィードバックが長く残る
    double getTailLengthSeconds() const override { return 8.0; }

    int getNumPrograms() override { return 1; }
    int getCurrentProgram() override { return 0; }
    void setCurrentProgram(int) override {}
    const juce::String getProgramName(int) override { return {}; }
    void changeProgramName(int, const juce::String&) override {}

    void getStateInformation(juce::MemoryBlock&) override;
    void setStateInformation(const void*, int) override;

    // ---- 画面 (WebView) から ----
    // パッドの押下 / 離鍵。メッセージスレッドから呼び、オーディオスレッドで拾う
    void uiPad(int pad, bool on);
    int paramCount() const { return (int) params.size(); }
    juce::RangedAudioParameter* param(int i) const { return params[(size_t) i]; }
    // ホストの MIDI と画面のどちらかで押されているパッド (bit i = パッド i)
    uint32_t heldMask() const { return held.load(); }

private:
    void setPad(int pad, int source, bool on);
    void render(float* l, float* r, int from, int to);

    StairHandle* engine = nullptr;
    std::vector<juce::RangedAudioParameter*> params;
    std::vector<double> sent;

    // 画面からのパッド操作 (単一生産者・単一消費者)
    juce::AbstractFifo uiFifo { 256 };
    std::array<int, 256> uiEvents {};

    // パッドごとの押下元 (0: MIDI, 1: 画面)
    std::array<std::array<bool, 2>, kPads> sources {};
    std::atomic<uint32_t> held { 0 };

    JUCE_DECLARE_NON_COPYABLE_WITH_LEAK_DETECTOR(StairProcessor)
};
