#include "Editor.h"
#include "BinaryData.h"

StairEditor::StairEditor(StairProcessor& p)
    : juce::AudioProcessorEditor(p),
      proc(p),
      web(juce::WebBrowserComponent::Options {}
              .withNativeIntegrationEnabled()
              .withResourceProvider([this](const auto& url) { return resource(url); })
              .withEventListener("stair", [this](const juce::var& v) { onEvent(v); }))
{
    shown.assign((size_t) proc.paramCount(), std::numeric_limits<double>::quiet_NaN());
    addAndMakeVisible(web);
    web.goToURL(juce::WebBrowserComponent::getResourceProviderRoot());
    setResizable(true, true);
    setResizeLimits(420, 480, 2400, 1600);
    setSize(960, 640);
    startTimerHz(30);
}

StairEditor::~StairEditor() { stopTimer(); }

void StairEditor::resized() { web.setBounds(getLocalBounds()); }

std::optional<juce::WebBrowserComponent::Resource> StairEditor::resource(const juce::String& url)
{
    // index.html はビルド時に BinaryData として埋め込む (Web 版と同じファイル)
    if (url == "/" || url == "/index.html")
    {
        const auto* d = reinterpret_cast<const std::byte*>(BinaryData::index_html);
        return juce::WebBrowserComponent::Resource { std::vector<std::byte>(d, d + BinaryData::index_htmlSize), "text/html" };
    }
    return std::nullopt;
}

void StairEditor::onEvent(const juce::var& m)
{
    const auto t = m["t"].toString();
    if (t == "ready")
    {
        // ページができたら今の値を全部送る
        ready = true;
        std::fill(shown.begin(), shown.end(), std::numeric_limits<double>::quiet_NaN());
        shownMask = 0xffffffff;
        timerCallback();
    }
    else if (t == "on" || t == "off")
    {
        proc.uiPad((int) m["pad"], t == "on");
    }
    else if (t == "param")
    {
        const int i = (int) m["i"];
        if (i < 0 || i >= proc.paramCount())
            return;
        auto* p = proc.param(i);
        const double v = (double) m["v"];
        shown[(size_t) i] = v;   // 画面が出した値なので送り返さない
        p->beginChangeGesture();
        p->setValueNotifyingHost(p->convertTo0to1((float) v));
        p->endChangeGesture();
    }
}

void StairEditor::sendParam(int i, double v)
{
    auto* o = new juce::DynamicObject();
    o->setProperty("t", "param");
    o->setProperty("i", i);
    o->setProperty("v", v);
    web.emitEventIfBrowserIsVisible("stair", juce::var(o));
}

void StairEditor::timerCallback()
{
    if (!ready)
        return;
    // ホスト側の変化 (オートメーション、プロジェクトの読み込み) を画面へ
    for (int i = 0; i < proc.paramCount(); ++i)
    {
        auto* p = proc.param(i);
        const double v = p->convertFrom0to1(p->getValue());
        if (!juce::exactlyEqual(v, shown[(size_t) i]))
        {
            shown[(size_t) i] = v;
            sendParam(i, v);
        }
    }
    // ホストの MIDI で押されているパッドを光らせる
    const auto mask = proc.heldMask();
    if (mask != shownMask)
    {
        shownMask = mask;
        auto* o = new juce::DynamicObject();
        o->setProperty("t", "held");
        o->setProperty("mask", (int) mask);
        web.emitEventIfBrowserIsVisible("stair", juce::var(o));
    }
}
