#ifndef WXD_PANEL_H
#define WXD_PANEL_H

#include "../wxd_types.h"

// --- Panel Functions ---
WXD_EXPORTED wxd_Panel_t*
wxd_Panel_Create(wxd_Window_t* parent, wxd_Id id, wxd_Point pos, wxd_Size size, wxd_Style_t style);

// Transient themed content, owned by its anchor's top-level window.
WXD_EXPORTED wxd_Window_t* wxd_TransientPopup_Create(wxd_Window_t* parent);
WXD_EXPORTED void wxd_TransientPopup_Show(wxd_Window_t* popup, wxd_Window_t* anchor, wxd_Window_t* focus);
WXD_EXPORTED void wxd_TransientPopup_Dismiss(wxd_Window_t* popup);

#endif // WXD_PANEL_H
