#include <wx/wxprec.h>
#include <wx/wx.h>
#include "../include/wxdragon.h"
#include "wxd_utils.h"
#include <wx/stattext.h> // Include wxStaticText header
#include <string>        // For std::string conversion
#include <wx/weakref.h>

class WxdStaticText : public wxStaticText {
    wxString original;
    int limit = -1, previous = -1;
    bool wrapping = false;
public:
    WxdStaticText(wxWindow* parent, wxWindowID id, const wxString& label, wxPoint pos, wxSize size, long style)
        : wxStaticText(parent, id, label, pos, size, style), original(label) {
        wxWeakRef<WxdStaticText> weak(this);
        parent->Bind(wxEVT_SIZE, [weak](wxSizeEvent& e) { if (weak) weak->Reflow(); e.Skip(); });
    }
    void SetLabel(const wxString& value) override {
        if (!wrapping) { original = value; previous = -1; }
        wxStaticText::SetLabel(value);
        if (!wrapping) Reflow();
    }
    void WrapTo(int width) { limit = width; previous = -1; Reflow(); }
    void Reflow() {
        if (wrapping || limit < 0 || !GetParent()) return;
        int width = std::min(limit, GetParent()->GetClientSize().x - FromDIP(28));
        if (width < FromDIP(72) || width == previous) return;
        previous = width; wrapping = true;
        wxStaticText::SetLabel(original); wxStaticText::Wrap(width);
        InvalidateBestSize(); wrapping = false;
    }
};

// Helper to convert wxPoint/wxSize if needed (assuming direct use is ok for now)
// inline wxPoint ToWxPoint(wxd_Point p) { return wxPoint(p.x, p.y); }
// inline wxSize ToWxSize(wxd_Size s) { return wxSize(s.width, s.height); }

extern "C" {

// --- StaticText Functions ---

WXD_EXPORTED wxd_StaticText_t*
wxd_StaticText_Create(wxd_Window_t* parent, wxd_Id id, const char* label, wxd_Point pos,
                      wxd_Size size, wxd_Style_t style)
{
    if (!parent) {
        // Maybe allow null parent for top-level? Unlikely for StaticText.
        return nullptr;
    }
    wxWindow* wx_parent = reinterpret_cast<wxWindow*>(parent);

    // Ensure label is valid UTF-8 before converting
    wxString wx_label = wxString::FromUTF8(label ? label : "");

    wxStaticText* stext = new WxdStaticText(wx_parent, id, wx_label, wxd_cpp_utils::to_wx(pos),
                                           wxd_cpp_utils::to_wx(size), style);

    return reinterpret_cast<wxd_StaticText_t*>(stext);
}

WXD_EXPORTED void
wxd_StaticText_Destroy(wxd_StaticText_t* stext)
{
    // Assumes stext is a top-level window, which is unlikely.
    // Usually, child windows are destroyed by their parents.
    // This function might not be needed if Drop logic handles parentage.
    // For safety, we'll call Destroy() which is safe for children too.
    if (!stext)
        return;
    wxStaticText* wx_stext = reinterpret_cast<wxStaticText*>(stext);
    wx_stext->Destroy();
}

WXD_EXPORTED void
wxd_StaticText_SetLabel(wxd_StaticText_t* stext, const char* label)
{
    if (!stext)
        return;
    wxStaticText* wx_stext = reinterpret_cast<wxStaticText*>(stext);
    wxString wx_label = wxString::FromUTF8(label ? label : "");
    wx_stext->SetLabel(wx_label);
}

WXD_EXPORTED int
wxd_StaticText_GetLabel(const wxd_StaticText_t* stext, char* buffer, size_t buffer_len)
{
    if (!stext)
        return -1;
    const wxStaticText* wx_stext = reinterpret_cast<const wxStaticText*>(stext);
    wxString label = wx_stext->GetLabel();
    return (int)wxd_cpp_utils::copy_wxstring_to_buffer(label, buffer, buffer_len);
}

WXD_EXPORTED void
wxd_StaticText_Wrap(wxd_StaticText_t* stext, int width)
{
    if (!stext)
        return;
    wxStaticText* wx_stext = reinterpret_cast<wxStaticText*>(stext);
    if (auto* modern = dynamic_cast<WxdStaticText*>(wx_stext)) modern->WrapTo(width);
    else wx_stext->Wrap(width);
}

} // extern "C"
