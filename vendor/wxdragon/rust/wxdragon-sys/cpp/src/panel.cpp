#include <wx/wxprec.h>
#include <wx/wx.h>
#include "wx/panel.h"
#include <wx/popupwin.h>
#include "wx/window.h" // Include parent class header if needed
#include "wxdragon.h"

extern "C" {

WXD_EXPORTED wxd_Window_t* wxd_TransientPopup_Create(wxd_Window_t* parent) {
    if (!parent) return nullptr;
    auto* popup = new wxPopupTransientWindow(reinterpret_cast<wxWindow*>(parent), wxBORDER_NONE);
    return reinterpret_cast<wxd_Window_t*>(popup);
}

WXD_EXPORTED void wxd_TransientPopup_Show(wxd_Window_t* popup, wxd_Window_t* anchor, wxd_Window_t* focus) {
    if (!popup || !anchor) return;
    auto* window = reinterpret_cast<wxPopupTransientWindow*>(popup);
    auto* origin = reinterpret_cast<wxWindow*>(anchor);
    window->Position(origin->ClientToScreen(wxPoint(0, 0)), origin->GetSize());
    window->Popup(reinterpret_cast<wxWindow*>(focus));
}

WXD_EXPORTED void wxd_TransientPopup_Dismiss(wxd_Window_t* popup) {
    if (popup) reinterpret_cast<wxPopupTransientWindow*>(popup)->Dismiss();
}

// Implement the creation function for wxPanel
WXD_EXPORTED wxd_Panel_t*
wxd_Panel_Create(wxd_Window_t* parent, wxd_Id id, wxd_Point pos, wxd_Size size, wxd_Style_t style)
{
    wxWindow* parentWin = (wxWindow*)parent; // Cast opaque parent pointer

    // Create wxWidgets objects for position and size
    wxPoint wxPos(pos.x, pos.y);
    wxSize wxSize(size.width, size.height);

    // Create the wxPanel instance
    // Note: wxPanel inherits from wxWindow, so the wxd_Window_t* parent is appropriate.
    wxPanel* panel = new wxPanel(parentWin, id, wxPos, wxSize, style);

    // Return the created panel pointer, cast to the opaque C type
    return (wxd_Panel_t*)panel;
}

// TODO: Add wxd_Panel_Destroy if needed, or confirm wxWindow_Destroy works.
// Panels, like most widgets, are typically destroyed when their parent frame/dialog is destroyed.
// Explicit destruction might only be necessary if dynamically removing panels without destroying the parent.
// For now, we assume parent-based destruction.

} // extern "C"
