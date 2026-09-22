#include "Processor.h"
#include "Editor.h"

StairProcessor::StairProcessor()
    : juce::AudioProcessor(BusesProperties().withOutput("Output", juce::AudioChannelSet::stereo(), true))
{
    // つまみの定義は engine/core/src/params.rs が正本。id は AU のパラメータ ID になるので変えない
    for (uint32_t i = 0; i < stair_param_count(); ++i)
    {
        char id[64], label[64];
        double mn, mx, step, def;
        stair_param_def(i, id, label, sizeof id, &mn, &mx, &step, &def);
        auto range = juce::NormalisableRange<float>((float) mn, (float) mx, (float) step);
        auto* p = new juce::AudioParameterFloat(juce::ParameterID { id, 1 }, label, range, (float) def);
        addParameter(p);
        params.push_back(p);
    }
    sent.assign(params.size(), std::numeric_limits<double>::quiet_NaN());
}

StairProcessor::~StairProcessor()
{
    if (engine != nullptr)
        stair_free(engine);
}

void StairProcessor::prepareToPlay(double sampleRate, int)
{
    // サンプルレートごとにエンジンを作り直す (メモリの確保はここだけ)
    if (engine != nullptr)
        stair_free(engine);
    engine = stair_new(sampleRate, (uint32_t) juce::Random::getSystemRandom().nextInt());
    std::fill(sent.begin(), sent.end(), std::numeric_limits<double>::quiet_NaN());
    for (auto& s : sources)
        s = { false, false };
    held = 0;
}

void StairProcessor::releaseResources() {}

bool StairProcessor::isBusesLayoutSupported(const BusesLayout& layouts) const
{
    return layouts.getMainOutputChannelSet() == juce::AudioChannelSet::stereo();
}

void StairProcessor::uiPad(int pad, bool on)
{
    const auto scope = uiFifo.write(1);
    if (scope.blockSize1 > 0)
        uiEvents[(size_t) scope.startIndex1] = on ? pad : -1 - pad;
}

void StairProcessor::setPad(int pad, int source, bool on)
{
    if (pad < 0 || pad >= kPads)
        return;
    auto& s = sources[(size_t) pad];
    const bool was = s[0] || s[1];
    s[(size_t) source] = on;
    const bool now = s[0] || s[1];
    if (!was && now)
        stair_note_on(engine, (uint32_t) pad);
    if (was && !now)
        stair_note_off(engine, (uint32_t) pad);
    const auto bit = 1u << pad;
    held = now ? (held.load() | bit) : (held.load() & ~bit);
}

void StairProcessor::render(float* l, float* r, int from, int to)
{
    // stair_render は長さの制限なし。128 サンプル境界の変調はエンジンが数えている
    if (to > from)
        stair_render(engine, l + from, r + from, (uint32_t) (to - from));
}

void StairProcessor::processBlock(juce::AudioBuffer<float>& buffer, juce::MidiBuffer& midi)
{
    juce::ScopedNoDenormals noDenormals;
    const int n = buffer.getNumSamples();
    buffer.clear();
    if (engine == nullptr || buffer.getNumChannels() < 2)
        return;

    // つまみ (ホストのオートメーションと画面の操作の両方がここに来る)
    for (size_t i = 0; i < params.size(); ++i)
    {
        const double v = params[i]->convertFrom0to1(params[i]->getValue());
        if (!juce::exactlyEqual(v, sent[i]))
        {
            stair_set_param(engine, (uint32_t) i, v);
            sent[i] = v;
        }
    }

    // 画面のパッドはブロックの先頭で
    const auto scope = uiFifo.read(uiFifo.getNumReady());
    for (int k = 0; k < scope.blockSize1; ++k)
    {
        const int e = uiEvents[(size_t) (scope.startIndex1 + k)];
        setPad(e >= 0 ? e : -1 - e, 1, e >= 0);
    }
    for (int k = 0; k < scope.blockSize2; ++k)
    {
        const int e = uiEvents[(size_t) (scope.startIndex2 + k)];
        setPad(e >= 0 ? e : -1 - e, 1, e >= 0);
    }

    // ホストの MIDI はサンプル位置で区切って反映する
    auto* l = buffer.getWritePointer(0);
    auto* r = buffer.getWritePointer(1);
    int pos = 0;
    for (const auto meta : midi)
    {
        const int at = juce::jlimit(0, n, meta.samplePosition);
        render(l, r, pos, at);
        pos = at;
        const auto m = meta.getMessage();
        if (m.isNoteOn())
            setPad(m.getNoteNumber() - kFirstNote, 0, true);
        else if (m.isNoteOff())
            setPad(m.getNoteNumber() - kFirstNote, 0, false);
        else if (m.isAllNotesOff() || m.isAllSoundOff())
            for (int p = 0; p < kPads; ++p)
                setPad(p, 0, false);
    }
    render(l, r, pos, n);
}

void StairProcessor::getStateInformation(juce::MemoryBlock& dest)
{
    // つまみの値だけを id で保存する (id は変えないので、将来つまみが増えても読める)
    juce::XmlElement xml("StairOne");
    for (auto* p : params)
        xml.setAttribute(p->paramID, p->convertFrom0to1(p->getValue()));
    copyXmlToBinary(xml, dest);
}

void StairProcessor::setStateInformation(const void* data, int size)
{
    if (auto xml = getXmlFromBinary(data, size))
        for (auto* p : params)
            if (xml->hasAttribute(p->paramID))
                p->setValueNotifyingHost(p->convertTo0to1((float) xml->getDoubleAttribute(p->paramID)));
}

juce::AudioProcessorEditor* StairProcessor::createEditor() { return new StairEditor(*this); }

juce::AudioProcessor* JUCE_CALLTYPE createPluginFilter() { return new StairProcessor(); }
