// 画面は Web 版と同じ index.html を WebView で出す。やり取りは JUCE の native integration のイベント 'stair' だけ
#pragma once

#include <juce_gui_extra/juce_gui_extra.h>
#include "Processor.h"

class StairEditor : public juce::AudioProcessorEditor, private juce::Timer
{
public:
    explicit StairEditor(StairProcessor&);
    ~StairEditor() override;
    void resized() override;

private:
    void timerCallback() override;
    void onEvent(const juce::var&);
    void sendParam(int i, double v);
    std::optional<juce::WebBrowserComponent::Resource> resource(const juce::String& url);

    StairProcessor& proc;
    std::vector<double> shown;
    uint32_t shownMask = 0xffffffff;
    bool ready = false;
    juce::WebBrowserComponent web;

    JUCE_DECLARE_NON_COPYABLE_WITH_LEAK_DETECTOR(StairEditor)
};
