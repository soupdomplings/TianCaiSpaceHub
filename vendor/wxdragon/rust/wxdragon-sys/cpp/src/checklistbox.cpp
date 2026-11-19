#include <wx/wxprec.h>
#include <wx/wx.h>
#include "../include/wxdragon.h"
#include <wx/checklst.h> // Include the correct header
#include <wx/window.h>
#include <wx/defs.h>   // For wxID_ANY, wxNOT_FOUND, wxDefaultPosition, wxDefaultSize
#include "wxd_utils.h" // For wxd_cpp_utils::copy_wxstring_to_buffer

#ifdef __WXMSW__
#include <wx/msw/wrapwin.h>
#include <wx/msw/dc.h>
#include <wx/graphics.h>
#include <memory>

namespace {
// Keep wxCheckListBox's native item storage, keyboard navigation, space toggles
// and accessible check-button states. Only its row presentation/hit area changes.
class HubChoiceList final : public wxCheckListBox {
public:
    HubChoiceList(wxWindow* parent, wxWindowID id, wxPoint pos, wxSize size, long style) {
        Create(parent, id, pos, size, wxArrayString(), style);
        UpdateHeight();
        Bind(wxEVT_LEFT_DOWN, [this](wxMouseEvent& e) {
            const int index = HitTest(e.GetPosition());
            if (index == wxNOT_FOUND || HasMultipleSelection()) { e.Skip(); return; }
            SetFocus();
            if (FindFocus() != this) return;
            SetSelection(index);
            EnsureVisible(index);
            Toggle(index);       // Also notifies accessibility clients.
            SendEvent(index);    // Same notification as the native space key.
        });
        Bind(wxEVT_SYS_COLOUR_CHANGED, [this](wxSysColourChangedEvent& e) {
            UpdateContrast(); Refresh(); e.Skip();
        });
        Bind(wxEVT_SIZE, [this](wxSizeEvent& e) { Refresh(false); e.Skip(); });
        UpdateContrast();
    }

    // Native list boxes use one height for all items. Reserve a second line
    // only for lists actually containing a title and a destination.
    void SetMultiline(bool value) {
        if (multiline != value) { multiline = value; UpdateHeight(); }
    }
    bool SetFont(const wxFont& font) override {
        const bool result = wxCheckListBox::SetFont(font);
        if (GetHandle()) UpdateHeight();
        return result;
    }
    bool MSWOnDraw(WXDRAWITEMSTRUCT* raw) override {
        const auto* s = reinterpret_cast<const DRAWITEMSTRUCT*>(raw);
        if (!s || s->itemID >= GetCount()) return false;
        wxDCTemp dc(reinterpret_cast<WXHDC>(s->hDC));
        dc.SetFont(GetFont());
        const bool focused = (s->itemState & ODS_FOCUS) && !(s->itemState & ODS_NOFOCUSRECT);
        const bool current = (s->itemState & ODS_SELECTED) && HasFocus();
        const bool checked = IsChecked(s->itemID);
        wxColour bg = contrast ? wxSystemSettings::GetColour(wxSYS_COLOUR_WINDOW) : GetBackgroundColour();
        const bool dark = bg.GetLuminance() < .36;
        const wxColour accent = contrast ? wxSystemSettings::GetColour(wxSYS_COLOUR_HIGHLIGHT)
                                         : (dark ? wxColour(147, 193, 247) : wxColour(23, 99, 188));
        const wxColour fg = contrast ? wxSystemSettings::GetColour(wxSYS_COLOUR_WINDOWTEXT) : GetForegroundColour();
        const wxColour secondary = contrast ? fg : (dark ? wxColour(185, 197, 212) : wxColour(83, 98, 117));
        const wxColour fill = current && !contrast ? (dark ? wxColour(43, 58, 76) : wxColour(231, 240, 250)) : bg;
        const int x = s->rcItem.left, y = s->rcItem.top, h = s->rcItem.bottom-y;
        const int right = GetClientSize().x, pad = FromDIP(10), d = FromDIP(17);
        dc.SetPen(*wxTRANSPARENT_PEN); dc.SetBrush(wxBrush(fill));
        dc.DrawRectangle(x, y, right-x, h);
        const int badge = right-pad-d;
        const auto text = GetString(s->itemID);
        const int split = text.Find('\n');
        const int lineH = dc.GetCharHeight();
        const int top = y + (h-lineH*(split == wxNOT_FOUND ? 1 : 2))/2;
        if (badge > x+2*pad) {
            wxDCClipper clip(dc, x+pad, y, badge-x-2*pad, h);
            dc.SetTextForeground(IsEnabled() ? fg : secondary);
            dc.DrawText(text.BeforeFirst('\n'), x+pad, top);
            if (split != wxNOT_FOUND) {
                dc.SetTextForeground(secondary);
                dc.DrawText(text.AfterFirst('\n'), x+pad, top+lineH);
            }
        }
        const int cy = y + (h-d)/2;
        const wxColour tick = contrast ? wxSystemSettings::GetColour(wxSYS_COLOUR_HIGHLIGHTTEXT)
                                       : (dark ? wxColour(25, 35, 49) : *wxWHITE);
        const auto ink = checked ? accent : secondary;
        const wxBrush brush(checked ? accent : fill);
        std::unique_ptr<wxGraphicsContext> gc(wxGraphicsContext::CreateFromNativeHDC(reinterpret_cast<WXHDC>(s->hDC)));
        if (gc) {
            gc->SetPen(wxPen(ink, FromDIP(1))); gc->SetBrush(brush);
            gc->DrawEllipse(badge, cy, d, d);
            if (checked) {
                gc->SetPen(wxPen(tick, FromDIP(2)));
                auto path = gc->CreatePath();
                path.MoveToPoint(badge+d*.24, cy+d*.51);
                path.AddLineToPoint(badge+d*.43, cy+d*.69);
                path.AddLineToPoint(badge+d*.77, cy+d*.32);
                gc->StrokePath(path);
            }
        } else {
            dc.SetPen(wxPen(ink)); dc.SetBrush(brush);
            dc.DrawEllipse(badge, cy, d, d);
            if (checked) {
                dc.SetPen(wxPen(tick, FromDIP(2)));
                dc.DrawLine(badge+d/4, cy+d/2, badge+d*4/9, cy+d*2/3);
                dc.DrawLine(badge+d*4/9, cy+d*2/3, badge+d*3/4, cy+d/3);
            }
        }
        gc.reset();
        if (focused || (current && contrast)) {
            dc.SetBrush(*wxTRANSPARENT_BRUSH); dc.SetPen(wxPen(accent));
            dc.DrawRoundedRectangle(1, y+1, right-2, h-2, FromDIP(5));
        }
        return true;
    }
    void MSWUpdateFontOnDPIChange(const wxSize& dpi) override {
        wxCheckListBox::MSWUpdateFontOnDPIChange(dpi);
        UpdateHeight();
    }
private:
    bool contrast = false, multiline = false;
    void UpdateHeight() {
        const auto h = GetCharHeight()*(multiline ? 2 : 1) + FromDIP(multiline ? 10 : 12);
        ::SendMessage(reinterpret_cast<HWND>(GetHandle()), LB_SETITEMHEIGHT, 0,
                      wxMax(h, FromDIP(multiline ? 44 : 33)));
    }
    void UpdateContrast() {
        HIGHCONTRASTW hc = {}; hc.cbSize = sizeof(hc);
        contrast = ::SystemParametersInfoW(SPI_GETHIGHCONTRAST, sizeof(hc), &hc, 0)
                   && (hc.dwFlags & HCF_HIGHCONTRASTON);
    }
};
}
#endif

// --- wxCheckListBox C Functions ---

extern "C" {

WXD_EXPORTED wxd_CheckListBox_t*
wxd_CheckListBox_Create(wxd_Window_t* parent, wxd_Id id, wxd_Point pos, wxd_Size size,
                        wxd_Style_t style)
{
    wxWindow* parentWin = reinterpret_cast<wxWindow*>(parent);
    wxPoint wxPos(pos.x, pos.y);
    wxSize wxSize(size.width, size.height);

    // For simplicity, we start with an empty choices array.
    // The wxCheckListBox constructor requires wxArrayString, but it can be empty.
#ifdef __WXMSW__
    auto* clbox = new HubChoiceList(parentWin, id, wxPos, wxSize, style);
#else
    auto* clbox = new wxCheckListBox(parentWin, id, wxPos, wxSize, wxArrayString(), style);
#endif

    return reinterpret_cast<wxd_CheckListBox_t*>(clbox);
}

WXD_EXPORTED void
wxd_CheckListBox_Append(wxd_CheckListBox_t* clbox, const char* item)
{
    wxCheckListBox* lb = (wxCheckListBox*)clbox;
    if (lb && item) {
        const auto label = wxString::FromUTF8(item);
        lb->Append(label);
#ifdef __WXMSW__
        if (label.Contains("\n")) static_cast<HubChoiceList*>(lb)->SetMultiline(true);
#endif
    }
}

WXD_EXPORTED void
wxd_CheckListBox_Insert(wxd_CheckListBox_t* clbox, const char* item, unsigned int pos)
{
    wxCheckListBox* lb = (wxCheckListBox*)clbox;
    if (lb && item) {
        const auto label = wxString::FromUTF8(item);
        lb->Insert(label, pos);
#ifdef __WXMSW__
        if (label.Contains("\n")) static_cast<HubChoiceList*>(lb)->SetMultiline(true);
#endif
    }
}

WXD_EXPORTED void
wxd_CheckListBox_Clear(wxd_CheckListBox_t* clbox)
{
    wxCheckListBox* wxClbox = reinterpret_cast<wxCheckListBox*>(clbox);
    if (wxClbox) {
        wxClbox->Clear();
#ifdef __WXMSW__
        static_cast<HubChoiceList*>(wxClbox)->SetMultiline(false);
#endif
    }
}

WXD_EXPORTED int
wxd_CheckListBox_GetSelection(wxd_CheckListBox_t* clbox)
{
    wxCheckListBox* wxClbox = reinterpret_cast<wxCheckListBox*>(clbox);
    if (wxClbox) {
        return wxClbox->GetSelection(); // Returns index or wxNOT_FOUND
    }
    return wxNOT_FOUND;
}

WXD_EXPORTED int
wxd_CheckListBox_GetStringSelection(const wxd_CheckListBox_t* clbox, char* buffer,
                                    size_t buffer_len)
{
    if (!clbox)
        return -1;
    const wxString sel = reinterpret_cast<const wxCheckListBox*>(clbox)->GetStringSelection();
    return (int)wxd_cpp_utils::copy_wxstring_to_buffer(sel, buffer, buffer_len);
}

WXD_EXPORTED void
wxd_CheckListBox_SetSelection(wxd_CheckListBox_t* clbox, int index, bool select)
{
    wxCheckListBox* wxClbox = reinterpret_cast<wxCheckListBox*>(clbox);
    if (wxClbox) {
        // Note: wxCheckListBox is single-selection by default. This behaves like wxListBox.
        wxClbox->SetSelection(index, select);
    }
}

WXD_EXPORTED int
wxd_CheckListBox_GetString(const wxd_CheckListBox_t* clbox, size_t index, char* buffer,
                           size_t buffer_len)
{
    if (!clbox)
        return -1;
    const wxCheckListBox* wxClbox = reinterpret_cast<const wxCheckListBox*>(clbox);
    if (index >= wxClbox->GetCount())
        return -1;
    const wxString str = wxClbox->GetString(index);
    return (int)wxd_cpp_utils::copy_wxstring_to_buffer(str, buffer, buffer_len);
}

WXD_EXPORTED unsigned int
wxd_CheckListBox_GetCount(wxd_CheckListBox_t* clbox)
{
    wxCheckListBox* wxClbox = reinterpret_cast<wxCheckListBox*>(clbox);
    if (wxClbox) {
        return wxClbox->GetCount();
    }
    return 0;
}

WXD_EXPORTED bool
wxd_CheckListBox_IsChecked(wxd_CheckListBox_t* clbox, unsigned int index)
{
    wxCheckListBox* wxClbox = reinterpret_cast<wxCheckListBox*>(clbox);
    if (wxClbox) {
        // Check index bounds? wxWidgets might assert or behave unexpectedly.
        if (index < wxClbox->GetCount()) {
            return wxClbox->IsChecked(index);
        }
    }
    return false; // Return false if index out of bounds or control invalid
}

WXD_EXPORTED void
wxd_CheckListBox_Check(wxd_CheckListBox_t* clbox, unsigned int index, bool check)
{
    wxCheckListBox* wxClbox = reinterpret_cast<wxCheckListBox*>(clbox);
    if (wxClbox) {
        if (index < wxClbox->GetCount()) {
            wxClbox->Check(index, check);
        }
    }
}

} // extern "C"
